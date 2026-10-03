#![forbid(unsafe_code)]

//! `ghaiw status` — readiness and what the worker is doing.

use crate::config::Config;
use crate::error::CliError;
use crate::{http_client, worker_env};

pub fn run(config: &Config) -> Result<(), CliError> {
    let base = match worker_env::load(&worker_env::default_path()) {
        Ok(env) => config.resolve_api_base(&env.api_base()),
        Err(_) => config.api_base.clone(),
    };
    // Readiness is the useful signal: it reports whether the container runtime
    // and work directory are actually usable, not merely that HTTP answers.
    let ready = http_client::request(&base, "GET", "/readyz", &[], None)?;
    if config.json {
        println!("{}", ready.body.trim());
        return Ok(());
    }
    let dependencies_ready = ready.body.contains("\"dependenciesReady\":true");
    println!("gha-indie-worker @ {base}");
    println!(
        "  ready         {}",
        if ready.is_success() { "yes" } else { "no" }
    );
    println!(
        "  dependencies  {}",
        if dependencies_ready {
            "ready"
        } else {
            "not ready — is the container runtime running?"
        }
    );
    Ok(())
}
