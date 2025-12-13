use crate::{
    agenix_rekey,
    commands::{
        runners::SnowCommand,
        util::{wrap, SnowConfig, VmConfigResolved},
    },
    git_add, rebuild, RebuildMode, Result, SnowError,
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
            )))
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
                    "nixosConfigurations.{}.config.formats.proxmox",
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

    // Resize disks according to nix vm config
    let command = SnowCommand::new(
        "ssh".to_string(),
        vec![
            &vm_config.proxmox_host,
            &format!(
                "qm disk resize {} virtio0 {}",
                vm_config.id, vm_config.resize_disk_to
            ),
        ],
        false,
    );
    log::info!(
        "Increasing disk size of disk \"vm_datastore:vm-{}-disk-0\" (virtio0) to {}...",
        vm_config.id,
        vm_config.resize_disk_to
    );
    command.run_silent().or_cleanup()?;

    // Boot the VM for the first time
    let command = SnowCommand::new(
        "ssh".to_string(),
        vec![
            &vm_config.proxmox_host,
            &format!("qm start {}", vm_config.id),
        ],
        false,
    );
    command.run_silent().or_cleanup()?;

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
        &Some(vm_configuration.to_string()),
        &RebuildMode::Boot,
        &None,
        &None,
        false,
        false,
        false,
    )?;
    unsafe {
        std::env::set_var("NIX_SSHOPTS", "");
    }

    // Reboot the VM so the new config with secret keys can become active
    let command = SnowCommand::new(
        "ssh".to_string(),
        vec![
            &vm_config.proxmox_host,
            &format!("qm reboot {}", vm_config.id),
        ],
        false,
    );
    log::info!("Rebooting {vm_configuration}...");
    command.run_silent()?;

    // Auto-accept the new VM key, and use the opportunity to run resize2fs
    let command = SnowCommand::new(
        "ssh".to_string(),
        vec![
            "-o",
            "StrictHostKeyChecking=accept-new",
            &snow_config.target_host.clone().unwrap(),
            "sudo",
            "resize2fs /dev/vda2",
        ],
        false,
    );
    command.run_silent()?;

    // OPTIONALLY: rebuild the local host to make its SSH handle available
    if rebuild_host {
        rebuild(
            &None,
            &RebuildMode::Switch,
            &None,
            &None,
            false,
            false,
            false,
        )?;
    }

    // OPTIONALLY: log the user into the new VM via SSH
    if login_after {
        let command = SnowCommand::new(
            "ssh".to_string(),
            vec![&snow_config.target_host.unwrap()],
            false,
        );
        command.run_silent()?;
    }
    log::info!("Done!");

    Ok(())
}
