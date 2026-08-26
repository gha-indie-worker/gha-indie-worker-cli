#![forbid(unsafe_code)]

use crate::config::Config;
use crate::error::CliError;

pub fn run(config: &Config) -> Result<(), CliError> {
    let body = serde_json::json!({
        "ok": true,
        "api_base": config.api_base,
    });
    if config.json {
        println!("{body}");
    } else {
        println!("ok {}", config.api_base);
    }
    Ok(())
}

