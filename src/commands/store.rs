use crate::commands::runners::SnowCommand;
use crate::util::Result;

pub(crate) fn referrers_closure(derivation: &str) -> Result<()> {
    let command = SnowCommand::new_nix(
        "nix-store".to_string(),
        vec!["-q", "--referrers-closure", derivation],
        false,
    );
    command.run_verbose()?;
    Ok(())
}
