#![forbid(unsafe_code)]

//! `giw health` — is the worker up?

use crate::config::Config;
use crate::error::CliError;
use crate::{http_client, runtime, worker_env};

/// The worker's base URL.
///
/// The launcher config knows the port the worker was actually started on; a
/// compiled-in default cannot, so it only applies when there is no config.
pub(crate) fn resolve_base(config: &Config) -> String {
    match worker_env::load(&worker_env::default_path()) {
        Ok(env) => config.resolve_api_base(&env.api_base()),
        Err(_) => config.api_base.clone(),
    }
}

pub fn run(config: &Config) -> Result<(), CliError> {
    let base = resolve_base(config);
    let response = http_client::request(&base, "GET", "/healthz", &[], None)?;
    let machine = if response.body.trim().is_empty() {
        serde_json::json!({ "ok": response.is_success(), "apiBase": base }).to_string()
    } else {
        response.body.trim().to_string()
    };
    let human = if response.is_success() {
        format!("ok {base}")
    } else {
        format!("unhealthy {base} (HTTP {})", response.status)
    };
    runtime::emit(config.json, human, &machine)?;
    if response.is_success() {
        Ok(())
    } else {
        Err(CliError::Command(format!(
            "worker reported HTTP {}",
            response.status
        )))
    }
}
