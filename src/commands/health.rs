#![forbid(unsafe_code)]

use crate::config::Config;
use crate::error::CliError;
use crate::runtime;

pub fn run(config: &Config) -> Result<(), CliError> {
    let body = serde_json::json!({
        "ok": true,
        "api_base": config.api_base,
    });
    runtime::emit(config.json, format!("ok {}", config.api_base), &body.to_string())
}
