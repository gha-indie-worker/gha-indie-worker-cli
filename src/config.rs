#![forbid(unsafe_code)]

#[path = "../generated/rust/env.rs"]
mod env;


use crate::env_map::{truthy, value, EnvMap};
use crate::error::CliError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub api_base: String,
    pub json: bool,
}

impl Config {
    pub fn from_env_map(env: &EnvMap) -> Result<Self, CliError> {
        let api_base = value(env, env::API_BASE)
            .unwrap_or("http://127.0.0.1:8080")
            .to_owned();
        if api_base.trim().is_empty() {
            return Err(CliError::Config("API base is empty".into()));
        }
        Ok(Self {
            api_base,
            json: truthy(env, env::JSON),
        })
    }
}
