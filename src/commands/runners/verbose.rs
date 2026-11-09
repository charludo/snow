use super::SnowCommand;
use crate::{SnowError, util::Result};
use std::process::{ExitStatus, Stdio};

impl SnowCommand {
    pub(crate) fn run_verbose(&self) -> Result<()> {
        self.spawn_verbose()?;
        Ok(())
    }

    /// Like `run_verbose`, but fails if the command exits unsuccessfully.
    pub(crate) fn run_verbose_checked(&self) -> Result<()> {
        let status = self.spawn_verbose()?;
        if !status.success() {
            return Err(SnowError::Command(format!("`{self}` exited with {status}")));
        }
        Ok(())
    }

    fn spawn_verbose(&self) -> Result<ExitStatus> {
        self.log();
        let mut child = self
            .process()
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(child.wait()?)
    }
}
