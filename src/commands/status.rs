#![forbid(unsafe_code)]

//! `ghaiw status` — readiness, and whether the worker can actually build.

use crate::config::Config;
use crate::error::CliError;
use crate::{http_client, runtime};

pub fn run(config: &Config) -> Result<(), CliError> {
    let base = super::health::resolve_base(config);
    // Readiness is the useful signal: it reports whether the container runtime
    // and work directory are usable, not merely that HTTP answers.
    let ready = http_client::request(&base, "GET", "/readyz", &[], None)?;
    let dependencies_ready = ready.body.contains("\"dependenciesReady\":true");
    let machine = if ready.body.trim().is_empty() {
        serde_json::json!({ "apiBase": base, "ready": ready.is_success() }).to_string()
    } else {
        ready.body.trim().to_string()
    };
    let human = format!(
        "gha-indie-worker @ {base}\n  ready         {}\n  dependencies  {}",
        if ready.is_success() { "yes" } else { "no" },
        if dependencies_ready {
            "ready"
        } else {
            "not ready — is the container runtime running?"
        }
    );
    runtime::emit(config.json, human, &machine)
}
