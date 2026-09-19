#![forbid(unsafe_code)]

//! `ghaiw worker up` — start the build worker on this machine.

use std::path::PathBuf;

use crate::config::Config;
use crate::error::CliError;
use crate::{proc, runtime, worker_env};

/// Environment variables that may carry a GitHub token.
///
/// The token is taken from the ambient environment rather than the config
/// file, so the credential is never written to disk by this CLI. The worker
/// reads `GH_PAT`, so a token found under another common name is forwarded
/// under that one.
const TOKEN_SOURCES: [&str; 3] = [
    "GH_PAT",
    "BUILD_SERVER_GIT_TOKEN",
    "GITHUB_PERSONAL_ACCESS_TOKEN",
];

fn worker_binary(env: &worker_env::WorkerEnv) -> Result<PathBuf, CliError> {
    if let Some(explicit) = std::env::var_os("GHAIW_WORKER_BIN") {
        return Ok(PathBuf::from(explicit));
    }
    if let Some(configured) = env.get("WORKER_BIN") {
        return Ok(PathBuf::from(configured));
    }
    Err(CliError::Config(
        "no worker binary: set GHAIW_WORKER_BIN, or WORKER_BIN in the worker env file".into(),
    ))
}

pub fn up(config: &Config) -> Result<(), CliError> {
    let (json, detach) = (config.json, config.detach);
    let env_path = worker_env::default_path();
    let env = worker_env::load(&env_path)?;
    let binary = worker_binary(&env)?;
    if !binary.is_file() {
        return Err(CliError::Config(format!(
            "worker binary {} does not exist",
            binary.display()
        )));
    }

    let mut vars: Vec<(String, String)> = env
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let mut token_source = None;
    for key in TOKEN_SOURCES {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                vars.push(("GH_PAT".to_string(), value));
                token_source = Some(key);
                break;
            }
        }
    }

    if !detach {
        runtime::emit_human(&format!(
            "starting worker from {} (config {})",
            binary.display(),
            env_path.display()
        ))?;
        return proc::inherit(&binary.to_string_lossy(), &[] as &[&str], &vars);
    }

    let log_path = env_path
        .parent()
        .unwrap_or(&PathBuf::from("."))
        .join("worker.log");
    let pid = proc::background(&binary.to_string_lossy(), &[] as &[&str], &vars, &log_path)?;
    let base = env.api_base();
    let machine = serde_json::json!({
        "pid": pid,
        "apiBase": base,
        "log": log_path.to_string_lossy(),
        // Named, never the value.
        "tokenSource": token_source,
    })
    .to_string();
    let credential = match token_source {
        // Reporting needs a credential; say whether one was found, so a
        // silently status-less run is not a surprise later.
        Some(source) => format!("reporting credential from {source}"),
        None => "no GitHub token found: builds run, but nothing is reported".to_string(),
    };
    let human = format!(
        "worker started (pid {pid})\n  api  {base}\n  log  {}\n  {credential}",
        log_path.display()
    );
    runtime::emit(json, human, &machine)
}
