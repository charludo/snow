use crate::{util::Result, verbose};
use std::process::Stdio;

use super::SnowCommand;

impl SnowCommand {
    pub(crate) fn run_silent(&self) -> Result<()> {
        if verbose() {
            return self.run_verbose();
        }
        self.log();

        let mut child = self
            .process()
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        child.wait()?;
        Ok(())
    }
}
