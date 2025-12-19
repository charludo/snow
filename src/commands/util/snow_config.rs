use crate::util::Result;
use crate::SnowError;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

use super::read_from_repl;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SnowConfig {
    pub(crate) tags: Vec<String>,
    pub(crate) use_remote_sudo: bool,
    pub(crate) ask_sudo_password: Option<bool>,
    pub(crate) build_on_target: bool,
    pub(crate) use_substitutes: bool,
    pub(crate) target_host: Option<String>,
    pub(crate) build_host: Option<String>,

    pub(crate) vm: Option<VmConfig>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VmConfig {
    pub(crate) id: Option<usize>,
    pub(crate) ip: Option<String>,
    pub(crate) proxmox_host: Option<String>,
    pub(crate) proxmox_image_store: Option<String>,
    pub(crate) resize_disk_to: Option<String>,
}

pub(crate) struct VmConfigResolved {
    pub(crate) id: usize,
    pub(crate) ip: String,
    pub(crate) proxmox_host: String,
    pub(crate) proxmox_image_store: String,
    pub(crate) resize_disk_to: String,
}

impl SnowConfig {
    pub(crate) fn get_snow_config(host: &str) -> Result<Self> {
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
            resize_disk_to: value
                .resize_disk_to
                .ok_or_else(|| SnowError::SnowConfig("missing resize_disk_to".to_string()))?,
        })
    }
}
