use std::{error::Error, fmt::Display};

pub type Result<T> = std::result::Result<T, SnowError>;

#[derive(Debug)]
pub(crate) enum SnowError {
    Nix(String),
    Env(String),
    SnowConfig(String),
    IO(std::io::Error),
}

impl Display for SnowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SnowError::Nix(e) => format!("Nix command failed with error: {e}"),
                SnowError::Env(e) => format!("Environment error: {e}"),
                SnowError::SnowConfig(e) => format!("Error parsing snow config: {e}"),
                SnowError::IO(e) => format!("Error in interaction with shell: {e}"),
            }
        )
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
