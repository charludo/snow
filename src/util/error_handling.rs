use inquire::InquireError;
use std::{error::Error, fmt::Display};

pub type Result<T> = std::result::Result<T, SnowError>;

#[derive(Debug)]
pub(crate) enum SnowError {
    Nix(String),
    Env(String),
    SnowConfig(String),
    Proxmox(String),
    Command(String),
    IO(std::io::Error),
    /// Already reported in full, so that only the exit code is left
    Reported,
}

impl Display for SnowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            SnowError::Nix(_) => "Nix command failed with error: ",
            SnowError::Env(_) => "Environment error: ",
            SnowError::SnowConfig(_) => "Error parsing snow config: ",
            SnowError::Proxmox(_) => "Proxmox error: ",
            SnowError::Command(_) => "Command failed: ",
            SnowError::IO(_) => "Error in interaction with shell: ",
            SnowError::Reported => "",
        };
        write!(f, "{kind}{}", self.message())
    }
}

impl Error for SnowError {}

impl From<serde_json::Error> for SnowError {
    fn from(value: serde_json::Error) -> Self {
        SnowError::SnowConfig(value.to_string())
    }
}

impl From<std::io::Error> for SnowError {
    fn from(value: std::io::Error) -> Self {
        SnowError::IO(value)
    }
}

impl From<InquireError> for SnowError {
    fn from(value: InquireError) -> Self {
        match value {
            // The prompt shows that it was cancelled
            InquireError::OperationCanceled | InquireError::OperationInterrupted => {
                SnowError::Reported
            }
            InquireError::IO(e) => SnowError::IO(e),
            e => SnowError::Env(e.to_string()),
        }
    }
}

impl SnowError {
    /// The error itself, without what kind it is
    pub(crate) fn message(&self) -> String {
        match self {
            SnowError::Nix(e)
            | SnowError::Env(e)
            | SnowError::SnowConfig(e)
            | SnowError::Proxmox(e)
            | SnowError::Command(e) => e.clone(),
            SnowError::IO(e) => e.to_string(),
            SnowError::Reported => "see above".to_string(),
        }
    }
}
