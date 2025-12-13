use crate::commands::runners::SnowCommand;
use crate::commands::util::wrap;
use crate::util::Result;

pub(crate) fn run(output: &Option<String>) -> Result<()> {
    let output = output.clone().unwrap_or("default".to_string());
    let command = SnowCommand::new_nix("nix".to_string(), vec!["run", &wrap(&output, true)], false);
    command.run_interactive()?;
    Ok(())
}

pub(crate) fn build(output: &Option<String>, just_hash: bool) -> Result<()> {
    let output = output.clone().unwrap_or("default".to_string());
    let command = SnowCommand::new_nix(
        "nix".to_string(),
        vec!["build", &wrap(&output, true)],
        false,
    );
    if just_hash {
        let hash = command.run_with_return_hash()?;
        log::info!("found hash: {}", hash);
        return Ok(());
    }
    command.run_verbose()
}
