use log::LevelFilter;

use crate::{util::Result, LOG_LEVEL};
use std::process::{Command, Stdio};

use super::SnowCommand;

impl SnowCommand {
    pub(crate) fn run_silent(&self) -> Result<()> {
        if LOG_LEVEL.get() == Some(&LevelFilter::Debug) {
            return self.run_verbose();
        }
        self.log();

        let (command, args) = self.get_final_args();
        let mut child = Command::new(command)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        child.wait()?;
        Ok(())
    }
}
