use crate::SnowError;
use crate::commands::runners::SnowCommand;
use crate::commands::util::{read_from_repl, wrap};
use crate::util::Result;
use std::path::Path;

pub(crate) fn cache(targets: &[String], to: &Option<String>) -> Result<()> {
    let to = to.as_deref().ok_or_else(|| {
        SnowError::Env("no cache configured: set snow.cache, $SNOW_CACHE or pass --to".to_string())
    })?;
    let store = match to.contains("://") {
        true => to.to_string(),
        false => format!("ssh-ng://{to}"),
    };
    let installables = resolve_installables(targets)?;

    let mut args = vec!["copy", "--to", &store];
    args.extend(installables.iter().map(String::as_str));
    let command = SnowCommand::new_nix("nix".to_string(), args, false);
    command.run_verbose_checked()
}

fn resolve_installables(targets: &[String]) -> Result<Vec<String>> {
    if targets.is_empty() {
        return std::fs::canonicalize("result")
            .map(|path| vec![path.to_string_lossy().to_string()])
            .map_err(|_| {
                SnowError::Env(
                    "./result does not exist, build something first or pass a path or output"
                        .to_string(),
                )
            });
    }

    let mut installables = Vec::new();
    let mut names = Vec::new();
    for target in targets {
        if Path::new(target).exists() {
            let path = std::fs::canonicalize(target)?;
            if !path.starts_with("/nix/store") {
                return Err(SnowError::Env(format!("{target} is not in the nix store")));
            }
            installables.push(path.to_string_lossy().to_string());
        } else if target.contains('#') || target.contains(':') {
            installables.push(target.clone());
        } else {
            names.push(target.as_str());
        }
    }
    if names.is_empty() {
        return Ok(installables);
    }

    let hosts = local_hosts()?;
    installables.extend(
        names
            .iter()
            .map(|name| match hosts.iter().any(|host| host == name) {
                true => wrap(
                    &format!("nixosConfigurations.\"{name}\".config.system.build.toplevel"),
                    true,
                ),
                false => wrap(name, true),
            }),
    );
    Ok(installables)
}

fn local_hosts() -> Result<Vec<String>> {
    let json = read_from_repl(
        ".",
        vec![
            "--json",
            "--apply",
            "outputs: builtins.attrNames (outputs.nixosConfigurations or { })",
        ],
    )?;
    Ok(serde_json::from_str(&json)?)
}
