use std::fs;

use inquire::Confirm;
use users::get_current_username;

use crate::SnowError;
use crate::options::RebuildMode;
use crate::util::Result;

use super::runners::SnowCommand;
use super::{agenix_rekey, fmt, git_add, rebuild};

// Minimal config that enables SSH, flakes, and trusted-users on a stock NixOS installation,
// importing the existing configuration so nothing else changes. Uses `nixos-rebuild test` so
// the change is live immediately but not written to the boot entry.
const PREPARE_NIX: &str = r#"{ config, pkgs, lib, ... }: {
  imports = [ /etc/nixos/configuration.nix ];
  services.openssh.enable = true;
  services.openssh.settings.PasswordAuthentication = true;
  nix.settings.trusted-users = [ "@wheel" ];
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
}"#;

pub(crate) fn assimilate_prepare() -> Result<()> {
    let tmp = "/tmp/snow-assimilate-prepare.nix";
    fs::write(tmp, PREPARE_NIX).map_err(SnowError::IO)?;

    let result = SnowCommand::new_nix(
        "nixos-rebuild".to_string(),
        vec!["test", "-I", &format!("nixos-config={}", tmp)],
        true,
    )
    .run_verbose();

    let _ = fs::remove_file(tmp);
    result?;

    let username_os = get_current_username().unwrap_or_default();
    let username = username_os.to_str().unwrap_or("user");

    let ips = SnowCommand::new("hostname".to_string(), vec!["-I"], false)
        .run_with_return()
        .unwrap_or_default();

    log::info!("SSH is now enabled with password authentication.");
    log::info!("From the managing host, run:");
    for ip in ips.split_whitespace() {
        log::info!("  snow assimilate {}@{} <nixos-config>", username, ip);
    }

    Ok(())
}

pub(crate) fn assimilate_run(target: &str, nixos_configuration: &str) -> Result<()> {
    // 1. Copy our SSH public key so all subsequent steps authenticate without a password
    log::info!("Copying SSH public key to {}...", target);
    SnowCommand::new(
        "ssh-copy-id".to_string(),
        vec!["-o", "StrictHostKeyChecking=accept-new", target],
        false,
    )
    .run_verbose()?;

    // 2. Fetch the target's host public key and store it for agenix
    log::info!("Fetching host public key from {}...", target);
    let pubkey = SnowCommand::new(
        "ssh".to_string(),
        vec![target, "cat", "/etc/ssh/ssh_host_ed25519_key.pub"],
        false,
    )
    .run_with_return()?;

    let pubkey_path = format!("hosts/{}/ssh_host_ed25519_key.pub", nixos_configuration);
    fs::write(&pubkey_path, pubkey.trim()).map_err(SnowError::IO)?;
    log::info!("Wrote host pubkey to {}", pubkey_path);

    // 3. Rekey agenix secrets to include the new host
    log::info!("Rekeying secrets for new host...");
    agenix_rekey(false, false)?;

    // 4. Generate and save hardware configuration
    log::info!("Generating hardware configuration on {}...", target);
    let hw_config = SnowCommand::new(
        "ssh".to_string(),
        vec![target, "nixos-generate-config", "--show-hardware-config"],
        false,
    )
    .run_with_return()?;

    let hw_path = format!("hosts/{}/hardware-configuration.nix", nixos_configuration);
    fs::write(&hw_path, &hw_config).map_err(SnowError::IO)?;
    log::info!("Wrote hardware configuration to {}", hw_path);

    // 5. Format, then stage everything (pubkey, rekeyed secrets, hardware config)
    fmt()?;
    git_add(false)?;

    // 6. Build and activate on next boot — avoids switching mid-session on the target
    log::info!(
        "Building and deploying {} to {} (boot mode)...",
        nixos_configuration,
        target
    );
    rebuild(
        &Some(nixos_configuration.to_string()),
        &RebuildMode::Boot,
        &Some(target.to_string()),
        &None,
        false,
        true,
        true,
        false,
        &None,
    )?;

    // 7. Confirm and reboot
    let do_reboot = Confirm::new(&format!("Deployment complete. Reboot {} now?", target))
        .with_default(true)
        .prompt()
        .map_err(|_| SnowError::Env("prompt cancelled".to_string()))?;

    if do_reboot {
        SnowCommand::new("ssh".to_string(), vec![target, "sudo", "reboot"], false).run_verbose()?;
    }

    Ok(())
}
