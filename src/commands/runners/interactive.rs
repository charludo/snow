use log::LevelFilter;

use super::SnowCommand;
use crate::{util::Result, SnowError, LOG_LEVEL};
use std::io::BufRead;
use std::io::BufReader;
use std::process::{Command, Stdio};

impl SnowCommand {
    pub(crate) fn run_interactive(&self) -> Result<()> {
        if LOG_LEVEL.get() == Some(&LevelFilter::Debug) {
            return self.run_verbose();
        }
        self.log();

        let (command, args) = self.get_final_args();
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        child.wait()?;

        let Some(errors) = child.stderr.take() else {
            return Ok(());
        };
        let mut error_count = 0;
        let mut error_line = String::new();
        let lines = BufReader::new(errors).lines();
        for line in lines {
            let line = line.unwrap();
            if line.contains("error:") {
                error_count += 1;
                error_line = line;
            }
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
    }
}
