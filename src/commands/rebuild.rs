use crate::commands::util::wrap;
use crate::util::Result;
use crate::{SnowError, LOG_LEVEL};
use gethostname::gethostname;
use inquire::Confirm;
use log::LevelFilter;
use users::get_current_username;

use super::runners::SnowCommand;
use super::util::SnowConfig;
use super::{exist_untracked, git_add, RebuildMode};

pub(crate) fn rebuild(
    nixos_configuration: &Option<String>,
    mode: &RebuildMode,
    target_host: &Option<String>,
    build_host: &Option<String>,
    build_on_target: bool,
    use_remote_sudo: bool,
) -> Result<()> {
    if exist_untracked()? {
        let answer = Confirm::new("Files exist which are untracked by git. If this rebuild depends on such a file, it will fail. Do you want to add them before proceeding?").with_default(true).prompt();
        match answer {
            Ok(true) => {
                git_add(false)?;
            }
            Ok(false) => {}
            Err(_) => std::process::exit(1),
        }
    }

    let hostname = gethostname();
    let (args, sudo) = match nixos_configuration {
        Some(nixos_configuration) => {
            let default_snow_config = SnowConfig::get_snow_config(nixos_configuration)?;
            let snow_config = SnowConfig {
                tags: default_snow_config.tags,
                use_remote_sudo: use_remote_sudo || default_snow_config.use_remote_sudo,
                build_on_target: build_on_target || default_snow_config.build_on_target,
                build_host: build_host
                    .clone()
                    .or(default_snow_config.build_host.to_owned()),
                target_host: target_host
                    .clone()
                    .or(default_snow_config.target_host.to_owned()),
                vm: None,
            };

            if (snow_config.target_host.is_none() && **nixos_configuration != *hostname)
                || snow_config.target_host != default_snow_config.target_host
            {
                let answer = Confirm::new(&format!(
                    "You are about to deploy the nixosConfiguration \"{}\" to the target host \"{}\", overwriting the default target location \"{}\" for this host. Are you absolutely certain that this is what you meant to do?",
                    nixos_configuration,
                    snow_config
                        .target_host.clone()
                        .unwrap_or(hostname.to_str().unwrap_or_default().to_string()),
                    default_snow_config
                        .target_host
                        .unwrap_or("[not specified]".to_string())
                ))
                    .with_default(false)
                    .with_help_message(&format!(
                            "!!! This will erase the configuration currently deployed to {} !!!",
                            &snow_config
                                .target_host.clone()
                                .unwrap_or(format!("your local machine, \"{}\"", hostname.to_str().unwrap_or_default()))
                        )
                    ).prompt();

                if !answer.is_ok_and(|x| x) {
                    std::process::exit(1);
                }
            }

            let mut args = vec![
                mode.to_string(),
                "--flake".to_string(),
                wrap(nixos_configuration, true),
            ];

            if let Some(ref target_host) = snow_config.target_host {
                args.push("--target-host".to_string());
                args.push(target_host.to_string());
            }

            if let Some(build_host) = snow_config.build_host {
                args.push("--build-host".to_string());
                args.push(build_host);
            } else if snow_config.build_on_target {
                if let Some(target_host) = snow_config.target_host.clone() {
                    args.push("--build-host".to_string());
                    args.push(target_host);
                } else {
                    return Err(SnowError::SnowConfig(
                        "\"build on target\" is specified, but no target host is given".to_string(),
                    ));
                }
            }

            if snow_config.use_remote_sudo {
                args.push("--sudo".to_string());
            }
            (args, false)
        }
        None => (
            vec![
                mode.to_string(),
                "--flake".to_string(),
                wrap(hostname.to_str().unwrap_or_default(), true),
            ],
            true,
        ),
    };

    let mut command = SnowCommand::new_nix(
        "nixos-rebuild".to_string(),
        args.iter().map(|x| x.as_str()).collect(),
        sudo,
    );

    match LOG_LEVEL.get() {
        Some(LevelFilter::Debug) => {
            command.append_arg("--show-trace");
            command.run_verbose()?
        }
        _ => command.run_progress(
            nixos_configuration
                .clone()
                .unwrap_or(hostname.to_str().unwrap_or_default().to_string()),
        )?,
    }
    Ok(())
}

pub(crate) fn home(home_configuration: &Option<String>) -> Result<()> {
    let username_os = get_current_username().unwrap_or_default();
    let username = username_os.to_str().unwrap_or_default();
    let hostname_os = gethostname();
    let hostname = hostname_os.to_str().unwrap_or_default();
    let default_target = if !username.is_empty() && !hostname.is_empty() {
        Some(format!("{username}@{hostname}"))
    } else {
        None
    };
    let target = match (&home_configuration, &default_target) {
        (Some(target), _) => target.as_str(),
        (None, Some(default_target)) => default_target,
        _ => {
            return Err(SnowError::Env(
                "failed to read username/hostname".to_string(),
            ));
        }
    };

    let mut command = SnowCommand::new_nix(
        "home-manager".to_string(),
        vec!["switch", "--flake", wrap(target, true).as_str()],
        false,
    );

    match LOG_LEVEL.get() {
        Some(LevelFilter::Debug) => {
            command.append_arg("--show-trace");
            command.run_verbose()?
        }
        _ => command.run_progress(target.to_string())?,
    }
    Ok(())
}
