use crate::commands::runners::SnowCommand;
use crate::commands::util::wrap;
use crate::util::Result;

pub(crate) fn run(output: &Option<String>) -> Result<()> {
    let output = output.clone().unwrap_or("default".to_string());
    let command = SnowCommand::new_nix("nix".to_string(), vec!["run", &wrap(&output, true)], false);
    command.run_interactive()?;
    Ok(())
}

pub(crate) fn build(output: &Option<String>) -> Result<()> {
    let output = output.clone().unwrap_or("default".to_string());
    let command = SnowCommand::new_nix(
        "nix".to_string(),
        vec!["build", &wrap(&output, true)],
        false,
    );
    command.run_verbose()?;
    Ok(())
}
