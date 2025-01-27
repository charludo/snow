use crate::util::Result;

use super::snow_command::SnowCommand;

pub(crate) fn agenix_update_masterkeys() -> Result<()> {
    let command = SnowCommand::new_agenix(
        "agenix".to_string(),
        vec!["--extra-flake-params", "?submodules=1", "update-masterkeys"],
    );
    command.run_interactive()?;
    Ok(())
}

pub(crate) fn agenix_edit(file: &str) -> Result<()> {
    let command = SnowCommand::new_agenix(
        "agenix".to_string(),
        vec!["--extra-flake-params", "?submodules=1", "edit", file],
    );
    command.run_interactive()?;
    Ok(())
}

pub(crate) fn agenix_rekey(force: bool, dummy: bool) -> Result<()> {
    let mut args = vec!["--extra-flake-params", "?submodules=1", "rekey"];
    if force {
        args.push("--force");
    }
    if dummy {
        args.push("--dummy");
    }
    let command = SnowCommand::new_agenix("agenix".to_string(), args);
    command.run_interactive()?;
    Ok(())
}

#[test]
fn test_agenix_update_masterkeys() {
    testing_logger::setup();
    let _ = agenix_update_masterkeys();
    crate::test_util::ensure_output("agenix --extra-flake-params ?submodules=1 update-masterkeys");
}

#[test]
fn test_agenix_edit() {
    testing_logger::setup();
    let _ = agenix_edit("help-im-not-real");
    crate::test_util::ensure_output(
        "agenix --extra-flake-params ?submodules=1 edit help-im-not-real",
    );
}

#[test]
fn test_agenix_rekey() {
    testing_logger::setup();
    let _ = agenix_rekey(true, false);
    crate::test_util::ensure_output("agenix --extra-flake-params ?submodules=1 rekey --force");
}
