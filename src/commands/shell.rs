use std::process::{Command, Stdio};

use crate::util::Result;

use super::{runners::SnowCommand, util::wrap};

fn get_interactive_args(
    default_command: String,
    default_args: Vec<String>,
    success_args: &mut Vec<String>,
) -> Result<(String, Vec<String>)> {
    if match Command::new("nix-your-shell")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => true,
        Err(e) => !matches!(e.kind(), std::io::ErrorKind::NotFound),
    } {
        success_args.insert(0, "fish".to_string());
        Ok(("nix-your-shell".to_string(), success_args.to_vec()))
    } else {
        Ok((default_command, default_args))
    }
}

pub(crate) fn shell(packages: &[String]) -> Result<()> {
    // We allow mixed shells. If no "#" is found in the package arg,
    // assume a nixpkgs package is meant.
    let mut default_args = packages
        .iter()
        .map(|p| {
            if p.contains("#") {
                p.to_owned()
            } else {
                format!("nixpkgs#{p}")
            }
        })
        .collect::<Vec<String>>()
        .to_vec();
    default_args.insert(0, "--impure".to_string()); // to pass UNFREE arg to nix shell
    default_args.insert(0, "--".to_string());
    default_args.insert(0, "shell".to_string());
    let mut success_args = default_args.clone();
    success_args.insert(0, "nix".to_string());

    let (command, args) = get_interactive_args("nix".to_string(), default_args, &mut success_args)?;

    let command = SnowCommand::new_nix(command, args.iter().map(|x| x.as_str()).collect(), false);
    unsafe {
        std::env::set_var("NIXPKGS_ALLOW_UNFREE", "1");
    }
    command.run_interactive()?;
    Ok(())
}

pub(crate) fn develop(shell_name: &Option<String>) -> Result<()> {
    let (command, mut args) = get_interactive_args(
        "nix".to_string(),
        vec!["develop".to_string()],
        &mut vec!["nix".to_string(), "develop".to_string()],
    )?;
    if let Some(name) = shell_name {
        args.push(wrap(name, true));
    }

    let command = SnowCommand::new_nix(command, args.iter().map(|x| x.as_str()).collect(), false);
    command.run_verbose()?;
    Ok(())
}
