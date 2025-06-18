use crate::{util::Result, SnowError};
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::process::{Command, Stdio};

use super::SnowCommand;

impl SnowCommand {
    pub(crate) fn run_with_return(&self) -> Result<String> {
        self.log();
        let (command, args) = self.get_final_args();
        let mut child = Command::new(command)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child.wait()?;

        let lines_err = BufReader::new(child.stderr.take().unwrap()).lines();
        for line in lines_err {
            let line = line.unwrap();
            if line.contains("error:") {
                return Err(SnowError::Nix(
                    line.replace("error:", "")
                        .replace("Definition values:", "")
                        .trim_start()
                        .trim_end()
                        .to_string(),
                ));
            }
        }

        let mut buf = String::new();
        let _ = BufReader::new(child.stdout.take().unwrap()).read_to_string(&mut buf);
        Ok(buf)
    }
}
