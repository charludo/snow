use log::LevelFilter;

use crate::commands::util::Progress;
use crate::{util::Result, SnowError, LOG_LEVEL};
use std::io::BufRead;
use std::io::BufReader;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use super::SnowCommand;

impl SnowCommand {
    pub(crate) fn run_progress(&self, name: String) -> Result<()> {
        if LOG_LEVEL.get() == Some(&LevelFilter::Debug) {
            return self.run_verbose();
        }
        self.log();

        let (command, args) = self.get_final_args();
        let mut child = Command::new(command)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;

        let lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let progress = Arc::new(Mutex::new(Progress::new(&name, 0)?));

        let progress_refresh = Arc::clone(&progress);
        let handle_refresh = thread::spawn(move || {
            // give sudo time to ask for a password
            std::thread::sleep(std::time::Duration::from_secs_f32(0.75));
            let exit_status = loop {
                match child.try_wait() {
                    Ok(None) => {
                        let mut progress = progress_refresh.lock().unwrap();
                        progress.refresh().unwrap();
                        std::mem::drop(progress);
                        std::thread::sleep(std::time::Duration::from_secs_f32(0.05));
                    }
                    other => break other,
                }
            };
            let mut progress = progress_refresh.lock().unwrap();
            progress.cleanup(exit_status.unwrap().unwrap()).unwrap();
            std::mem::drop(progress);
        });

        let progress_parse = Arc::clone(&progress);
        let handle_parse = thread::spawn(move || {
            let mut error_count = 0;
            let mut error_line = String::new();

            for line in lines {
                let mut progress = progress_parse.lock().unwrap();
                let line = line.unwrap();
                if line.contains("error:") {
                    error_count += 1;
                    error_line = line;
                } else if line.contains(" will be built:") {
                    progress.add_derivations(&line);
                } else if line.contains(" will be fetched (") {
                    progress.add_fetched(&line);
                } else if line.starts_with("building") || line.starts_with("copying") {
                    progress.progress();
                } else if line.trim_start().starts_with("/nix/store") {
                    progress.add_task();
                }
                std::mem::drop(progress);
                if error_count == 2 {
                    return Err(SnowError::Nix(
                        error_line
                            .replace("error:", "")
                            .replace("Definition values:", "")
                            .trim_start()
                            .trim_end()
                            .to_string(),
                    ));
                }
            }

            Ok(())
        });

        handle_refresh.join().unwrap();
        handle_parse.join().unwrap()
    }

    pub(crate) fn run_progress_import(&self) -> Result<()> {
        if LOG_LEVEL.get() == Some(&LevelFilter::Debug) {
            return self.run_verbose();
        }
        self.log();

        let (command, args) = self.get_final_args();
        let mut child = Command::new(command)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let progress = Arc::new(Mutex::new(Progress::new("import vm", 99)?));

        let progress_refresh = Arc::clone(&progress);
        let handle_refresh = thread::spawn(move || {
            let exit_status = loop {
                match child.try_wait() {
                    Ok(None) => {
                        let mut progress = progress_refresh.lock().unwrap();
                        progress.refresh().unwrap();
                        std::mem::drop(progress);
                        std::thread::sleep(std::time::Duration::from_secs_f32(0.05));
                    }
                    other => break other,
                }
            };
            let mut progress = progress_refresh.lock().unwrap();
            progress.cleanup(exit_status.unwrap().unwrap()).unwrap();
            std::mem::drop(progress);
        });

        let progress_parse = Arc::clone(&progress);
        let handle_parse = thread::spawn(move || {
            for line in lines {
                let mut progress = progress_parse.lock().unwrap();
                let line = line.unwrap();
                if line.starts_with("progress") {
                    progress.progress();
                }
                std::mem::drop(progress);
            }
        });

        handle_refresh.join().unwrap();
        handle_parse.join().unwrap();
        Ok(())
    }
}
