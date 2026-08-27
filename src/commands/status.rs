#![forbid(unsafe_code)]

use crate::config::Config;
use crate::error::CliError;

pub fn run(config: &Config) -> Result<(), CliError> {
    let body = serde_json::json!({
        "service": "gha-indie-worker",
        "api_base": config.api_base,
    });
    if config.json {
        println!("{body}");
    } else {
        println!("gha-indie-worker @ {}", config.api_base);
    }
    Ok(())
}
