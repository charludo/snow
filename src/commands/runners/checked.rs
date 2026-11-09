use super::SnowCommand;
use crate::{SnowError, util::Result};

impl SnowCommand {
    /// Run the command, returning its stdout, or its stderr as error if it exits unsuccessfully.
    pub(crate) fn run_checked(&self) -> Result<String> {
        self.log();
        let output = self.process().output()?;
        if !output.status.success() {
            return Err(SnowError::Command(format!(
                "`{}`: {}",
                self,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
