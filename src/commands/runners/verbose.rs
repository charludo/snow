use super::SnowCommand;
use crate::util::Result;
use std::process::{Command, Stdio};

impl SnowCommand {
    pub(crate) fn run_verbose(&self) -> Result<()> {
        self.log();
        let (command, args) = self.get_final_args();
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        child.wait()?;
        Ok(())
    }
}
