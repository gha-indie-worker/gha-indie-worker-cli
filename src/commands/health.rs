#![forbid(unsafe_code)]

//! `ghaiw health` — is the worker up?

use crate::config::Config;
use crate::error::CliError;
use crate::{http_client, worker_env};

pub fn run(config: &Config) -> Result<(), CliError> {
    // The launcher config knows the port the worker was started on; the flag
    // contract's default cannot.
    let base = match worker_env::load(&worker_env::default_path()) {
        Ok(env) => config.resolve_api_base(&env.api_base()),
        Err(_) => config.api_base.clone(),
    };
    let response = http_client::request(&base, "GET", "/healthz", &[], None)?;
    if config.json {
        println!("{}", response.body.trim());
    } else if response.is_success() {
        println!("ok {base}");
    } else {
        println!("unhealthy {base} (HTTP {})", response.status);
    }
    if response.is_success() {
        Ok(())
    } else {
        Err(CliError::Command(format!(
            "worker reported HTTP {}",
            response.status
        )))
    }
}
