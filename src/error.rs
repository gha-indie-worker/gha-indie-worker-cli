#![forbid(unsafe_code)]

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("command failed: {0}")]
    Command(String),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Config(_) => 2,
            Self::Command(_) => 1,
        }
    }
}

