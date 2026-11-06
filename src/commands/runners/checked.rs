use super::SnowCommand;
use crate::{SnowError, util::Result};
use std::process::Command;

impl SnowCommand {
    /// Run the command, returning its stdout, or its stderr as error if it exits unsuccessfully.
    pub(crate) fn run_checked(&self) -> Result<String> {
        self.log();
        let (command, args) = self.get_final_args();
        let output = Command::new(command).args(args).output()?;
        if !output.status.success() {
            return Err(SnowError::Proxmox(format!(
                "`{}` failed: {}",
                self,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
