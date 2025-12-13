use crate::commands::runners::SnowCommand;
use crate::commands::util::wrap;
use crate::util::Result;

pub(crate) fn run(output: &str) -> Result<()> {
    let command = SnowCommand::new_nix("nix".to_string(), vec!["run", &wrap(output, true)], false);
    command.run_interactive()?;
    Ok(())
}

pub(crate) fn build(output: &str) -> Result<()> {
    let command =
        SnowCommand::new_nix("nix".to_string(), vec!["build", &wrap(output, true)], false);
    command.run_progress(output.to_string())?;
    Ok(())
}
