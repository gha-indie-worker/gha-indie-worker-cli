#![forbid(unsafe_code)]

use crate::args::Invocation;
use crate::error::CliError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub api_base: String,
    pub json: bool,
}

impl Config {
    pub fn load(invocation: &Invocation) -> Result<Self, CliError> {
        let api_base = invocation
            .api_base
            .clone()
            .or_else(|| std::env::var("GHA_INDIE_WORKER_API_BASE").ok())
            .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
        if api_base.trim().is_empty() {
            return Err(CliError::Config("API base is empty".into()));
        }
        Ok(Self {
            api_base,
            json: invocation.json || std::env::var("GHA_INDIE_WORKER_JSON").is_ok(),
        })
    }
}

