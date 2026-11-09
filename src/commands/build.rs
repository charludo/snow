use crate::commands::runners::SnowCommand;
use crate::commands::util::wrap;
use crate::commands::{cache_store, copy_to_cache};
use crate::util::Result;

pub(crate) fn run(output: &Option<String>) -> Result<()> {
    let output = output.clone().unwrap_or("default".to_string());
    let command = SnowCommand::new_nix("nix".to_string(), vec!["run", &wrap(&output, true)], false);
    command.run_verbose()?;
    Ok(())
}

pub(crate) fn build(
    output: &Option<String>,
    just_hash: bool,
    cache: Option<Option<&str>>,
) -> Result<()> {
    let output = output.clone().unwrap_or("default".to_string());
    let installable = wrap(&output, true);
    let command = SnowCommand::new_nix("nix".to_string(), vec!["build", &installable], false);
    if just_hash {
        let hash = command.run_with_return_hash()?;
        log::info!("found hash: {}", hash);
        return Ok(());
    }
    let Some(store) = cache.map(cache_store).transpose()? else {
        return command.run_verbose();
    };
    command.run_verbose_checked()?;
    copy_to_cache(&store, None, &[installable]).run_verbose_checked()
}
