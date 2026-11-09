use crate::SnowError;
use crate::commands::runners::SnowCommand;
use crate::commands::util::{read_from_repl, wrap};
use crate::util::Result;
use std::path::Path;

pub(crate) fn cache(targets: &[String], to: &Option<String>) -> Result<()> {
    let store = cache_store(to.as_deref())?;
    let installables = resolve_installables(targets)?;
    copy_to_cache(&store, None, &installables).run_verbose_checked()
}

/// The store URI of the cache `to`, or else of the one configured in snow.cache
pub(crate) fn cache_store(to: Option<&str>) -> Result<String> {
    let to = match to {
        Some(to) => to.to_string(),
        None => std::env::var("SNOW_CACHE")
            .ok()
            .filter(|to| !to.is_empty())
            .ok_or_else(|| {
                SnowError::Env(
                    "no cache configured: set snow.cache, $SNOW_CACHE or pass one explicitly"
                        .to_string(),
                )
            })?,
    };
    Ok(match to.contains("://") {
        true => to,
        false => format!("ssh-ng://{to}"),
    })
}

/// Copy `installables` to `store`, from the store `from` instead of the local one if given
pub(super) fn copy_to_cache(
    store: &str,
    from: Option<&str>,
    installables: &[impl AsRef<str>],
) -> SnowCommand {
    let mut args = vec!["copy", "--to", store];
    if let Some(from) = from {
        args.extend(["--from", from]);
    }
    args.extend(installables.iter().map(AsRef::as_ref));
    SnowCommand::new_nix("nix".to_string(), args, false)
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
