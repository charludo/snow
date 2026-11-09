use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGTSTP};
use signal_hook::low_level::emulate_default_handler;
use std::fs::File;
use std::io::Write;
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

static DRAWN: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_drawn(drawn: bool) {
    DRAWN.store(drawn, Ordering::SeqCst);
}

pub(crate) fn show_cursor(visible: bool) {
    write_stderr(if visible { b"\x1b[?25h" } else { b"\x1b[?25l" });
}

fn write_stderr(bytes: &[u8]) {
    let mut stderr = ManuallyDrop::new(unsafe { File::from_raw_fd(2) });
    let _ = stderr.write_all(bytes);
}

fn restore_terminal(exiting: bool) {
    if exiting && DRAWN.load(Ordering::SeqCst) {
        write_stderr(b"\r\n");
    }
    show_cursor(true);
}

pub(crate) fn guard_terminal() {
    static GUARD: Once = Once::new();
    GUARD.call_once(|| {
        for signal in [SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGTSTP] {
            // Safety: the handler only makes system calls, which is async-signal-safe
            let _ = unsafe {
                signal_hook::low_level::register(signal, move || {
                    restore_terminal(signal != SIGTSTP);
                    let _ = emulate_default_handler(signal);
                })
            };
        }
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal(true);
            previous(info);
        }));
    });
}
