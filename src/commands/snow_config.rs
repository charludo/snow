use crate::util::Result;
use crate::SnowError;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

use super::util::read_from_repl;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SnowConfig {
    pub(super) tags: Vec<String>,
    pub(super) use_remote_sudo: bool,
    pub(super) build_on_target: bool,
    pub(super) target_host: Option<String>,
    pub(super) build_host: Option<String>,

    pub(super) vm: Option<VmConfig>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct VmConfig {
    pub(super) id: Option<usize>,
    pub(super) ip: Option<String>,
    pub(super) proxmox_host: Option<String>,
    pub(super) proxmox_image_store: Option<String>,
    pub(super) resize_disk_by: Option<String>,
}

pub(super) struct VmConfigResolved {
    pub(super) id: usize,
    pub(super) ip: String,
    pub(super) proxmox_host: String,
    pub(super) proxmox_image_store: String,
    pub(super) resize_disk_by: String,
}

impl SnowConfig {
    pub(super) fn get_snow_config(host: &str) -> Result<Self> {
        match read_from_repl(
            &format!("nixosConfigurations.{host}.config.snow"),
            vec!["--json"],
        ) {
            Ok(snow_config_raw) => Ok(serde_json::from_str(&snow_config_raw)?),
            Err(e) => Err(SnowError::Nix(format!(
                "could not read snow config for host {host}: {e}"
            ))),
        }
    }
}

impl TryFrom<VmConfig> for VmConfigResolved {
    type Error = SnowError;

    fn try_from(value: VmConfig) -> Result<Self> {
        Ok(Self {
            id: value
                .id
                .ok_or_else(|| SnowError::SnowConfig("missing id".to_string()))?,
            ip: value
                .ip
                .ok_or_else(|| SnowError::SnowConfig("missing ip".to_string()))?,
            proxmox_host: value
                .proxmox_host
                .ok_or_else(|| SnowError::SnowConfig("missing proxmox_host".to_string()))?,
            proxmox_image_store: value
                .proxmox_image_store
                .ok_or_else(|| SnowError::SnowConfig("missing proxmox_image_store".to_string()))?,
            resize_disk_by: value
                .resize_disk_by
                .ok_or_else(|| SnowError::SnowConfig("missing resize_disk_by".to_string()))?,
        })
    }
}
