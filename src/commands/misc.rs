use crate::{options::RebuildMode, util::Result};

use super::{rebuild, runners::SnowCommand};

pub(crate) fn fmt() -> Result<()> {
    let command = SnowCommand::new_nix("nix".to_string(), vec!["fmt"], false);
    command.run_progress("fmt".to_string())?;
    Ok(())
}

pub(crate) fn clean(rebuild_after: bool) -> Result<()> {
    let mut command = SnowCommand::new_nix("nix-collect-garbage".to_string(), vec!["-d"], true);
    command.run_verbose()?;

    command.requires_sudo = false;
    command.run_verbose()?;

    if rebuild_after {
        return rebuild(&None, &RebuildMode::Boot, &None, &None, false, false);
    }

    Ok(())
}

pub(crate) fn update(input: &Option<String>) -> Result<()> {
    let mut args = vec!["flake", "update"];
    if let Some(input_arg) = input {
        args.push(input_arg);
    }

    let command = SnowCommand::new_nix("nix".to_string(), args, false);
    command.run_verbose()?;
    Ok(())
}

#[test]
fn test_fmt() {
    testing_logger::setup();
    let _ = fmt();
    crate::test_util::ensure_output("nix fmt");
}
