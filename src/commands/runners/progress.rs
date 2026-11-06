use log::LevelFilter;

use crate::commands::util::Progress;
use crate::{LOG_LEVEL, SnowError, util::Result};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use super::SnowCommand;

impl SnowCommand {
    pub(crate) fn run_progress(&self, name: String) -> Result<()> {
        if LOG_LEVEL.get() == Some(&LevelFilter::Debug) {
            return self.run_verbose_checked();
        }

        // Password prompts (local sudo, nixos-rebuild's --ask-sudo-password) come before the
        // first line of output, and must not be drawn over
        let may_prompt =
            self.requires_sudo || self.args.iter().any(|arg| arg == "--ask-sudo-password");

        let mut error_count = 0;
        let mut error_line = String::new();
        let status = self.run_with_bar(&name, 0, may_prompt, false, |progress, line| {
            if line.contains("error:") {
                // The first error line is often just "error:", with the message following
                error_count += 1;
                if error_count <= 2 {
                    error_line = line.to_string();
                }
            } else if error_count > 0 {
                // Details of the error, which can list store paths as well
            } else if line.contains(" will be built:") {
                progress.add_derivations(line);
            } else if line.contains(" will be fetched (") {
                progress.add_fetched(line);
            } else if line.starts_with("building '")
                || (line.starts_with("copying path '") && line.contains("' from '"))
            {
                // Only builds and substitutions are listed as tasks, copies to other hosts aren't
                progress.progress();
            } else if line.trim_start().starts_with("/nix/store") {
                progress.add_task();
            }
        })?;

        if status.success() {
            return Ok(());
        }
        let error_line = error_line
            .replace("error:", "")
            .replace("Definition values:", "")
            .trim()
            .to_string();
        Err(SnowError::Nix(if error_line.is_empty() {
            format!("`{self}` exited with {status}")
        } else {
            error_line
        }))
    }

    pub(crate) fn run_progress_import(&self) -> Result<()> {
        if LOG_LEVEL.get() == Some(&LevelFilter::Debug) {
            return self.run_verbose_checked();
        }

        // "progress 42% (read 1234 bytes, duration 5 sec)"
        let status = self.run_with_bar("import vm", 100, false, true, |progress, line| {
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

    /// Run the command, feeding each line of its stderr (or stdout) to `on_line` while a progress
    /// bar is shown. With `wait_for_output`, the bar is only drawn once the first line arrives.
    fn run_with_bar(
        &self,
        name: &str,
        total: usize,
        wait_for_output: bool,
        from_stdout: bool,
        mut on_line: impl FnMut(&mut Progress, &str),
    ) -> Result<ExitStatus> {
        self.log();

        let (command, args) = self.get_final_args();
        let mut command = Command::new(command);
        command.args(args);
        let mut child = if from_stdout {
            command.stdout(Stdio::piped()).stderr(Stdio::null())
        } else {
            command.stdout(Stdio::null()).stderr(Stdio::piped())
        }
        .spawn()?;
        let output: Box<dyn Read + Send> = match from_stdout {
            true => Box::new(child.stdout.take().unwrap()),
            false => Box::new(child.stderr.take().unwrap()),
        };

        let progress = Mutex::new(match wait_for_output {
            true => None,
            false => Some(Progress::new(name, total)?),
        });
        let finished = AtomicBool::new(false);

        let read = || -> Result<()> {
            // Read until the end, as the command blocks once the pipe is full
            for line in BufReader::new(output).lines().map_while(|line| line.ok()) {
                let mut progress = progress.lock().unwrap();
                let progress = match progress.as_mut() {
                    Some(progress) => progress,
                    None => progress.insert(Progress::new(name, total)?),
                };
                on_line(progress, &line);
            }
            Ok(())
        };
        thread::scope(|scope| {
            scope.spawn(|| {
                while !finished.load(Ordering::Relaxed) {
                    if let Some(progress) = progress.lock().unwrap().as_mut() {
                        let _ = progress.refresh();
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            });
            let result = read();
            finished.store(true, Ordering::Relaxed);
            result
        })?;

        let status = child.wait()?;
        let mut progress = match progress.into_inner().unwrap() {
            Some(progress) => progress,
            None => Progress::new(name, total)?,
        };
        progress.cleanup(status)?;
        Ok(status)
    }
}
