use crate::commands::util::{Phase, Progress, Screen, Tone, askpass_envs};
use crate::{SnowError, util::Result, verbose};
use parking_lot::{Mutex, MutexGuard};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, PipeReader};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use super::{SnowCommand, nix_error};

const ERROR_CONTEXT_LINES: usize = 20;

/// A command of a parallel run, shown as a row named `name`
pub(crate) struct Job {
    pub(crate) name: String,
    pub(crate) command: SnowCommand,
    /// Run after all other jobs, on its own
    pub(crate) last: bool,
    /// Shown until it deploys
    pub(crate) note: String,
    /// Whether the note is shown while it is queued as well, instead of "queued"
    pub(crate) note_queued: bool,
    pub(crate) deploy: Deploy,
}

/// When a job building a host starts to deploy it, from which on it is shown as a bar
pub(crate) enum Deploy {
    /// It only builds
    Never,
    /// It only deploys, what was built already
    Now,
    /// Once it copies the system to the target host, or activates it
    Later(Option<String>),
}

pub(crate) struct JobResult {
    pub(crate) name: String,
    pub(crate) result: Result<()>,
    pub(crate) output: Vec<String>,
}

impl JobResult {
    pub(crate) fn new(name: &str, result: Result<()>) -> Self {
        Self {
            name: name.to_string(),
            result,
            output: vec![],
        }
    }
}

/// A password prompt in progress, holds the prompt lock, and keeps the screen paused until dropped
struct Prompting<'a> {
    screen: &'a Mutex<Screen>,
    _prompt: MutexGuard<'a, ()>,
}

impl<'a> Prompting<'a> {
    fn start(screen: &'a Mutex<Screen>, prompt: &'a Mutex<()>) -> Self {
        let prompt = prompt.lock();
        screen.lock().pause();
        Self {
            screen,
            _prompt: prompt,
        }
    }
}

impl Drop for Prompting<'_> {
    fn drop(&mut self) {
        self.screen.lock().resume();
    }
}

#[derive(Default)]
struct NixOutput {
    error_count: usize,
    error_line: String,
}

impl NixOutput {
    fn error(&mut self, line: &str) -> bool {
        if line.starts_with("cannot build on '") {
            return true;
        }
        if line.contains("error:") {
            self.error_count += 1;
            if self.error_count <= 2 {
                self.error_line = line.to_string();
            }
        }
        self.error_count > 0
    }

    fn parse(&mut self, progress: &mut Progress, line: &str) {
        if self.error(line) {
            return;
        }
        if line.contains(" will be built:") {
            progress.add_derivations(line);
        } else if line.contains(" will be fetched (") {
            progress.add_fetched(line);
        } else if line.starts_with("building '")
            || (line.starts_with("copying path '") && line.contains("' from '"))
        {
            progress.progress();
        } else if line.trim_start().starts_with("/nix/store") {
            progress.add_task();
        }
    }

    fn result(self, command: &SnowCommand, status: ExitStatus) -> Result<()> {
        if status.success() {
            return Ok(());
        }
        let error_line = nix_error(&self.error_line);
        Err(SnowError::Nix(if error_line.is_empty() {
            format!("`{command}` exited with {status}")
        } else {
            error_line
        }))
    }
}

impl SnowCommand {
    pub(crate) fn run_progress(&self, name: String) -> Result<()> {
        if verbose() {
            return self.run_verbose_checked();
        }

        let mut nix = NixOutput::default();
        let status =
            self.run_with_bar(&name, 0, false, |progress, line| nix.parse(progress, line))?;
        nix.result(self, status)
    }

    pub(crate) fn run_progress_import(&self) -> Result<()> {
        if verbose() {
            return self.run_verbose_checked();
        }

        // "progress 42% (read 1234 bytes, duration 5 sec)"
        let status = self.run_with_bar("import vm", 100, true, |progress, line| {
            if let Some(percent) = line
                .strip_prefix("progress ")
                .and_then(|rest| rest.split('%').next()?.parse().ok())
            {
                progress.set_done(percent);
            }
        })?;

        if !status.success() {
            return Err(SnowError::Command(format!("`{self}` exited with {status}")));
        }
        Ok(())
    }

    /// Password prompts (local sudo, nixos-rebuild's --ask-sudo-password) come before the first line of output
    fn may_prompt(&self) -> bool {
        self.requires_sudo || self.args.iter().any(|arg| arg == "--ask-sudo-password")
    }

    /// Spawn the command, returning a reader of its stdout, or of its stderr and stdout combined
    fn spawn_output(&self, from_stdout: bool) -> Result<(PipeReader, Child)> {
        let (reader, writer) = std::io::pipe()?;
        let (stdout, stderr): (Stdio, Stdio) = match from_stdout {
            true => (writer.into(), Stdio::null()),
            false => (writer.try_clone()?.into(), writer.into()),
        };
        let child = self
            .process()
            .envs(askpass_envs())
            .stdin(match self.may_prompt() {
                true => Stdio::inherit(),
                false => Stdio::null(),
            })
            .stdout(stdout)
            .stderr(stderr)
            .spawn()?;
        Ok((reader, child))
    }

    fn run_with_bar(
        &self,
        name: &str,
        total: usize,
        from_stdout: bool,
        mut on_line: impl FnMut(&mut Progress, &str),
    ) -> Result<ExitStatus> {
        let mut screen = Screen::new(false);
        let row = screen.add_row(name, false, false);
        screen.progress(row).set_total(total);
        let screen = Mutex::new(screen);
        let prompt = Mutex::new(());
        let status = with_redraw(&screen, || {
            self.run_row(&screen, row, None, from_stdout, &prompt, |screen, line| {
                on_line(screen.progress(row), line)
            })
        });
        screen.into_inner().finish();
        status
    }

    fn run_row(
        &self,
        screen: &Mutex<Screen>,
        row: usize,
        prefix: Option<&str>,
        from_stdout: bool,
        prompt: &Mutex<()>,
        mut on_line: impl FnMut(&mut Screen, &str),
    ) -> Result<ExitStatus> {
        let mut prompting = self.may_prompt().then(|| Prompting::start(screen, prompt));
        self.log();

        let (output, mut child) = self
            .spawn_output(from_stdout)
            .inspect_err(|_| screen.lock().finish_row(row, false))?;
        screen.lock().progress(row);

        for line in BufReader::new(output).lines().map_while(|line| line.ok()) {
            prompting = None;
            let mut screen = screen.lock();
            screen.add_output(prefix, &line);
            if let Some(warning) = unreachable_builder(&line) {
                screen.message(Tone::Warn, None, &warning);
            }
            on_line(&mut screen, &line);
        }
        drop(prompting);

        let status = child.wait()?;
        screen.lock().finish_row(row, status.success());
        Ok(status)
    }
}

impl Job {
    fn run(&self, screen: &Mutex<Screen>, row: usize, prompt: &Mutex<()>) -> JobResult {
        let mut nix = NixOutput::default();
        let mut output = VecDeque::with_capacity(ERROR_CONTEXT_LINES);
        let mut paths = 0;
        let status = self.command.run_row(
            screen,
            row,
            Some(&self.name),
            false,
            prompt,
            |screen, line| {
                if output.len() == ERROR_CONTEXT_LINES {
                    output.pop_front();
                }
                output.push_back(line.to_string());

                let count = copy_count(line);
                paths = count.unwrap_or(paths);
                if screen.phase(row) == Phase::Building && self.deploys(line) {
                    screen.start_deploy(row, paths);
                }
                match screen.phase(row) {
                    Phase::Deploying if nix.error(line) => {}
                    Phase::Deploying => match count {
                        Some(count) => screen.progress(row).set_total(count),
                        None if line.starts_with("copying path '") && line.contains("' to '") => {
                            screen.progress(row).progress();
                        }
                        None => {}
                    },
                    Phase::Bar | Phase::Building => nix.parse(screen.progress(row), line),
                }
            },
        );
        let result = status.and_then(|status| nix.result(&self.command, status));

        let mut screen = screen.lock();
        if screen.phase(row) == Phase::Building {
            let note = match (&result, &self.deploy) {
                (Err(_), _) => "failed to build",
                (Ok(()), Deploy::Never) => "built",
                (Ok(()), _) => "deployed",
            };
            screen.set_note(row, Some(note));
        }
        JobResult {
            name: self.name.clone(),
            result,
            output: output.into(),
        }
    }

    fn deploys(&self, line: &str) -> bool {
        let Deploy::Later(target) = &self.deploy else {
            return false;
        };
        let copied_to_target = target.as_ref().is_some_and(|target| {
            line.starts_with("copying path '") && line.contains(&format!("://{target}'"))
        });
        copied_to_target || line.contains("activating the configuration")
    }
}

fn unreachable_builder(line: &str) -> Option<String> {
    let (builder, reason) = line.strip_prefix("cannot build on '")?.split_once('\'')?;
    let reason = reason.trim_start_matches(':').trim();
    let reason = reason.strip_prefix("error:").unwrap_or(reason).trim();
    let warning = format!("Builder {builder} is unreachable, building without it");
    Some(match reason.is_empty() {
        true => warning,
        false => format!("{warning}: {reason}"),
    })
}

fn copy_count(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("copying ")?;
    let count = rest
        .strip_suffix(" paths...")
        .or(rest.strip_suffix(" path..."))?;
    count.parse().ok()
}

pub(crate) struct Live {
    screen: Mutex<Screen>,
    prompt: Mutex<()>,
}

pub(crate) struct Jobs<'a> {
    live: &'a Live,
    sender: Sender<(usize, Job)>,
    last: Mutex<Vec<(usize, Job)>>,
    reserved: Mutex<BTreeMap<String, usize>>,
}

impl Live {
    pub(crate) fn run<T>(work: impl FnOnce(&Live) -> T) -> T {
        let live = Live {
            screen: Mutex::new(Screen::new(true)),
            prompt: Mutex::new(()),
        };
        let result = with_redraw(&live.screen, || work(&live));
        live.screen.into_inner().finish();
        result
    }

    pub(crate) fn screen(&self) -> MutexGuard<'_, Screen> {
        self.screen.lock()
    }

    pub(crate) fn run_step(&self, row: usize, job: Job) -> JobResult {
        job.run(&self.screen, row, &self.prompt)
    }

    pub(crate) fn run_jobs<T>(
        &self,
        max_jobs: usize,
        produce: impl FnOnce(&Jobs) -> T,
    ) -> (T, Vec<JobResult>) {
        let (sender, receiver) = mpsc::channel();
        let receiver = Mutex::new(receiver);
        let results = Mutex::new(Vec::new());
        let run = |row: usize, job: Job| {
            let phase = match job.deploy {
                Deploy::Now => Phase::Deploying,
                Deploy::Never | Deploy::Later(_) => Phase::Building,
            };
            self.screen.lock().set_job(row, phase, &job.note);
            let result = job.run(&self.screen, row, &self.prompt);
            if let Err(e) = &result.result {
                let mut screen = self.screen.lock();
                screen.message(Tone::Bad, Some(&result.name), &e.message());
            }
            results.lock().push(result);
        };

        let (produced, last) = thread::scope(|scope| {
            for _ in 0..max_jobs.max(1) {
                scope.spawn(|| {
                    loop {
                        // Ends once the jobs are queued, and all are taken
                        let next = receiver.lock().recv();
                        let Ok((row, job)) = next else { break };
                        run(row, job);
                    }
                });
            }
            let jobs = Jobs {
                live: self,
                sender,
                last: Mutex::new(Vec::new()),
                reserved: Mutex::new(BTreeMap::new()),
            };
            (produce(&jobs), jobs.last.into_inner())
        });
        for (row, job) in last {
            let status = match results.lock().is_empty() {
                true => format!("Rebuilding {}", job.name),
                false => format!("Rebuilding {} last", job.name),
            };
            self.screen().set_status(status);
            run(row, job);
        }
        (produced, results.into_inner())
    }
}

impl Jobs<'_> {
    pub(crate) fn send(&self, mut jobs: Vec<Job>) {
        jobs.sort_by_key(|job| !job.command.may_prompt());
        for job in jobs {
            let mut screen = self.live.screen.lock();
            let row = match self.reserved.lock().remove(&job.name) {
                Some(row) => row,
                None => screen.add_row(&job.name, true, false),
            };
            screen.set_note(row, job.note_queued.then_some(job.note.as_str()));
            drop(screen);
            match job.last {
                true => self.last.lock().push((row, job)),
                false => {
                    let _ = self.sender.send((row, job));
                }
            }
        }
    }

    /// Add rows for `names` ahead of their jobs, below the group added last, building as part of it until those are queued
    pub(crate) fn reserve(&self, names: &[&str]) {
        let mut screen = self.live.screen.lock();
        let mut reserved = self.reserved.lock();
        for name in names {
            let row = screen.add_row(name, true, true);
            screen.set_note(row, Some("building"));
            reserved.insert(name.to_string(), row);
        }
    }

    /// Finish the row reserved for `name` without a job, with `note` as its outcome
    pub(crate) fn complete(&self, name: &str, note: &str) {
        if let Some(row) = self.reserved.lock().remove(name) {
            self.live.screen().complete_row(row, true, note);
        }
    }
}

/// Redraw `screen` every 50ms while `work` runs
fn with_redraw<T>(screen: &Mutex<Screen>, work: impl FnOnce() -> T) -> T {
    struct Stop<'a>(&'a AtomicBool);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    let finished = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::Relaxed) {
                screen.lock().draw();
                thread::sleep(Duration::from_millis(50));
            }
        });
        let _stop = Stop(&finished);
        work()
    })
}

#[test]
fn test_unreachable_builder() {
    assert_eq!(
        unreachable_builder(
            "cannot build on 'ssh-ng://abc': error: failed to start SSH connection to 'abc'"
        )
        .as_deref(),
        Some(
            "Builder ssh-ng://abc is unreachable, building without it: failed to start SSH \
             connection to 'abc'"
        )
    );
    assert_eq!(
        unreachable_builder("building '/nix/store/abc.drv'..."),
        None
    );
}
