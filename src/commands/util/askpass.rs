use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::env;
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, symlink};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, exit};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

pub(crate) const ASKPASS_NAME: &str = "snow-askpass";
const SOCKET_NAME: &str = "socket";
const SOCKET_ENV: &str = "SNOW_ASKPASS_SOCKET";
const ORIGINAL_ENV: &str = "SNOW_ASKPASS_ORIGINAL";

static SOCKET_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
static SSH_PENDING: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn presence_pending() -> bool {
    SSH_PENDING.load(Ordering::Relaxed) > 0
}

pub(crate) fn askpass_envs() -> Vec<(&'static str, OsString)> {
    if cfg!(test) {
        return Vec::new();
    }
    let dir = SOCKET_DIR.get_or_init(|| {
        listen()
            .inspect_err(|e| log::debug!("cannot detect security key presence checks: {e}"))
            .ok()
    });
    let Some(dir) = dir else {
        return Vec::new();
    };
    let mut envs = vec![
        ("SSH_ASKPASS", dir.join(ASKPASS_NAME).into_os_string()),
        (SOCKET_ENV, dir.join(SOCKET_NAME).into_os_string()),
    ];
    // Under another snow's askpass, it is the original one
    if let Some(original) = env::var_os(ORIGINAL_ENV).or_else(|| env::var_os("SSH_ASKPASS")) {
        envs.push((ORIGINAL_ENV, original));
    }
    envs
}

fn listen() -> io::Result<PathBuf> {
    let base = env::var_os("XDG_RUNTIME_DIR").map_or_else(env::temp_dir, PathBuf::from);
    let dir = base.join(format!("snow-askpass-{}", std::process::id()));
    DirBuilder::new().mode(0o700).create(&dir)?;
    symlink(env::current_exe()?, dir.join(ASKPASS_NAME))?;
    let listener = UnixListener::bind(dir.join(SOCKET_NAME))?;
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            SSH_PENDING.fetch_add(1, Ordering::Relaxed);
            thread::spawn(move || {
                let _ = stream.read_to_end(&mut Vec::new());
                SSH_PENDING.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Ok(dir)
}

pub(crate) fn askpass_cleanup() {
    if let Some(Some(dir)) = SOCKET_DIR.get() {
        let _ = fs::remove_file(dir.join(ASKPASS_NAME));
        let _ = fs::remove_file(dir.join(SOCKET_NAME));
        let _ = fs::remove_dir(dir);
    }
}

pub(crate) fn run_askpass(args: Vec<OsString>) -> ! {
    let original = env::var_os(ORIGINAL_ENV).filter(|original| !original.is_empty());
    let original_command = |original: &OsString| {
        let mut command = Command::new(original);
        command
            .args(&args)
            .env("SSH_ASKPASS", original)
            .env_remove(SOCKET_ENV)
            .env_remove(ORIGINAL_ENV);
        command
    };

    if env::var_os("SSH_ASKPASS_PROMPT").is_none_or(|prompt| prompt != "none") {
        let Some(original) = original else { exit(1) };
        let error = original_command(&original).exec();
        eprintln!("snow: cannot run askpass {}: {error}", original.display());
        exit(1);
    }

    let _connection = env::var_os(SOCKET_ENV).and_then(|socket| UnixStream::connect(socket).ok());
    let mut notification = original.and_then(|original| original_command(&original).spawn().ok());
    match Signals::new([SIGTERM, SIGINT, SIGHUP]) {
        Ok(mut signals) => _ = signals.forever().next(),
        Err(_) => loop {
            thread::park();
        },
    }
    if let Some(notification) = &mut notification {
        let _ = notification.kill();
        let _ = notification.wait();
    }
    exit(0)
}
