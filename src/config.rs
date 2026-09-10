#![forbid(unsafe_code)]

#[path = "../generated/rust/env.rs"]
mod env;

use crate::env_map::{truthy, value, EnvMap};
use crate::error::CliError;

const INDIEBUILD_CONFIG_ENV: &str = "INDIEBUILD_CONFIG";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub api_base: String,
    pub json: bool,
    pub indiebuild_config: String,
}

impl Config {
    pub fn from_env_map(env_map: &EnvMap) -> Result<Self, CliError> {
        let api_base = value(env_map, env::API_BASE)
            .unwrap_or("http://127.0.0.1:8080")
            .to_owned();
        if api_base.trim().is_empty() {
            return Err(CliError::Config("API base is empty".into()));
        }
        let indiebuild_config = value(env_map, INDIEBUILD_CONFIG_ENV)
            .unwrap_or(".indiebuild.toml")
            .to_owned();
        if indiebuild_config.trim().is_empty() {
            return Err(CliError::Config("IndieBuild config path is empty".into()));
        }
        Ok(Self {
            api_base,
            json: truthy(env_map, env::JSON),
            indiebuild_config,
        })
    }
}
