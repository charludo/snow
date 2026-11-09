use crate::{
    commands::util::{read_from_repl, wrap},
    options::RebuildMode,
    util::Result,
};

use super::{Flags, rebuild, runners::SnowCommand};

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
        return rebuild(&[], &[], &RebuildMode::Boot, &Flags::default());
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

pub(crate) fn check() -> Result<()> {
    let wrapped = wrap("", true);
    let args = vec!["flake", "check", &wrapped];
    let command = SnowCommand::new_nix("nix".to_string(), args, false);
    command.run_verbose()?;
    Ok(())
}

pub(crate) fn tags(hostname: &str) -> Result<()> {
    let raw = read_from_repl(
        &format!("nixosConfigurations.{hostname}.config.snow.tags"),
        vec!["--json"],
    )?;
    let tags: Vec<String> = serde_json::from_str(&raw)?;
    for tag in tags {
        println!("{tag}");
    }
    Ok(())
}

pub(crate) fn config(hostname: &str) -> Result<()> {
    let raw = read_from_repl(
        &format!("nixosConfigurations.{hostname}.config.snow"),
        vec!["--json"],
    )?;
    let config: serde_json::Value = serde_json::from_str(&raw)?;
    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

#[test]
fn test_fmt() {
    testing_logger::setup();
    let _ = fmt();
    crate::test_util::ensure_output("nix fmt");
}
