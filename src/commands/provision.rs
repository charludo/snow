use crate::{
    Flags, RebuildMode, Result, SnowError, agenix_rekey,
    commands::{
        runners::SnowCommand,
        util::{SnowConfig, VmConfigResolved, wrap},
    },
    git_add, rebuild, respec, set_vm_status,
};
use std::{thread, time::Duration};

trait OrCleanup {
    fn or_cleanup(self) -> Self;
}
impl<T> OrCleanup for crate::Result<T> {
    fn or_cleanup(self) -> Self {
        if self.is_ok() {
            return self;
        }
        cleanup()?;
        self
    }
}

fn cleanup() -> Result<()> {
    std::fs::remove_dir_all("result")?;
    Ok(())
}

pub(crate) fn provision(
    vm_configuration: &str,
    login_after: bool,
    rebuild_host: bool,
) -> crate::Result<()> {
    let snow_config = SnowConfig::get_snow_config(vm_configuration)?;
    let vm_config: VmConfigResolved = match snow_config.vm {
        Some(vm_config) => vm_config.try_into()?,
        None => {
            return Err(SnowError::SnowConfig(format!(
                "VM settings are not configured for host \"{vm_configuration}\""
            )));
        }
    };

    // Rekey secrets with a dummy kwy for the new host
    agenix_rekey(false, true)?;
    git_add(false)?;

    // Generate the vm through nix build
    let command = SnowCommand::new_nix(
        "nix".to_string(),
        vec![
            "build",
            &wrap(
                &format!(
                    "nixosConfigurations.{}.config.system.build.images.proxmox",
                    vm_configuration
                ),
                true,
            ),
        ],
        false,
    );
    command
        .run_progress(vm_configuration.to_string())
        .or_cleanup()?;

    // Copy the result to the proxmox iso dir
    let command = SnowCommand::new(
        "cp".to_string(),
        vec![
            &format!("result/vzdump-qemu-{}.vma.zst", vm_configuration),
            &format!(
                "{}/vzdump-qemu-{}-2024_06_01-10_00_00.vma.zst",
                vm_config.proxmox_image_store, vm_config.id
            ),
        ],
        false,
    );
    log::info!("Copying the VM image to the proxmox host...");
    command.run_verbose().or_cleanup()?;

    // Import the VM on the proxmox host
    let command = SnowCommand::new(
        "ssh".to_string(),
        vec![
            &vm_config.proxmox_host,
            &format!(
                "qmrestore /mnt/pve/proxmox_images/template/iso/vzdump-qemu-{}-2024_06_01-10_00_00.vma.zst {} --unique true",
                vm_config.id, vm_config.id
            ),
        ],
        false,
    );
    command.run_progress_import().or_cleanup()?;

    //Remove no-longer needed files
    log::info!("Performing cleanup tasks...");
    std::fs::remove_dir_all("result")?;
    std::fs::remove_file(format!(
        "{}/vzdump-qemu-{}-2024_06_01-10_00_00.vma.zst",
        vm_config.proxmox_image_store, vm_config.id
    ))?;

    // Grow the disk and put the VM under HA, which boots it on a node its affinity rule allows
    respec(vm_configuration, false, true)?;
    if vm_config.ha.is_none() {
        set_vm_status(&vm_config.proxmox_host, vm_config.id, "start")?;
    }

    // Remove any stale known_hosts entries so re-provisioning doesn't fail on key mismatch
    let _ =
        SnowCommand::new("ssh-keygen".to_string(), vec!["-R", &vm_config.ip], false).run_silent();
    if let Some(ref target_host) = snow_config.target_host {
        let _ =
            SnowCommand::new("ssh-keygen".to_string(), vec!["-R", target_host], false).run_silent();
    }

    // Obtain the public key
    let command = SnowCommand::new("ssh-keyscan".to_string(), vec![&vm_config.ip], false);
    log::info!("Waiting for {vm_configuration} to come online to obtain its public ssh key...");
    let pub_key = loop {
        // Give machine time to boot
        thread::sleep(Duration::from_millis(5000));
        if let Some(key) = command
            .run_with_return()
            .or_cleanup()?
            .lines()
            .find(|line| line.starts_with(&vm_config.ip) && line.contains("ssh-ed25519"))
        {
            break format!(
                "{} {}",
                key.replace(&vm_config.ip, "").trim_start(),
                vm_configuration
            );
        };
    };

    // Auto-accept the new VM key, now that the VM is known to be up
    if let Some(ref target_host) = snow_config.target_host {
        let command = SnowCommand::new(
            "ssh".to_string(),
            vec![
                "-o",
                "StrictHostKeyChecking=accept-new",
                target_host,
                "true",
            ],
            false,
        );
        command.run_silent()?;
    }

    // Save pubkey and add to git
    std::fs::write(
        format!("vms/keys/ssh_host_{vm_configuration}_ed25519_key.pub"),
        pub_key,
    )?;
    git_add(false)?;

    // Rekey secrets for the new host, with real keys this time
    agenix_rekey(false, false)?;
    git_add(false)?;

    // Rebuild the host, with correct secrets this time
    unsafe {
        std::env::set_var(
            "NIX_SSHOPTS",
            "-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null",
        );
    }
    rebuild(
        &[vm_configuration.to_string()],
        &[],
        &RebuildMode::Boot,
        &Flags::default(),
    )?;
    unsafe {
        std::env::set_var("NIX_SSHOPTS", "");
    }

    // Reboot the VM so the new config with secret keys can become active
    log::info!("Rebooting {vm_configuration}...");
    set_vm_status(&vm_config.proxmox_host, vm_config.id, "reboot")?;

    // OPTIONALLY: rebuild the local host to make its SSH handle available
    if rebuild_host {
        rebuild(&[], &[], &RebuildMode::Switch, &Flags::default())?;
    }

    // OPTIONALLY: log the user into the new VM via SSH
    if login_after {
        let target_host = snow_config.target_host.ok_or_else(|| {
            SnowError::SnowConfig(format!("{vm_configuration} has no targetHost to log into"))
        })?;
        SnowCommand::new("ssh".to_string(), vec![&target_host], false).run_verbose()?;
    }
    log::info!("Done!");

    Ok(())
}
