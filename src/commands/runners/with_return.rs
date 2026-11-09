use crate::{SnowError, util::Result};

use super::{SnowCommand, nix_error};

impl SnowCommand {
    pub(crate) fn run_with_return(&self) -> Result<String> {
        self.log();
        let output = self.process().output()?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        if let Some(line) = stderr.lines().rev().find(|line| line.contains("error:")) {
            return Err(SnowError::Nix(nix_error(line)));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub(crate) fn run_with_return_hash(&self) -> Result<String> {
        self.log();
        let output = self.process().output()?;

        for line in String::from_utf8_lossy(&output.stderr).lines() {
            if line.contains("   got:   ") {
                return Ok(line.replace("got:", "").trim().to_string());
            }
        }

        Err(SnowError::Nix("no missing hash found".to_string()))
    }
}
