#![forbid(unsafe_code)]

//! `giw tunnel` — the public ingress for GitHub's webhook deliveries.
//!
//! A *named* tunnel, not a quick tunnel: quick tunnels get a new random
//! hostname every run, which would mean re-registering the webhook on every
//! restart. A named tunnel keeps one stable hostname and one credentials file.

use std::path::PathBuf;

use crate::config::Config;
use crate::error::CliError;
use crate::{proc, runtime, worker_env};

fn cloudflared_home() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".cloudflared")
}

/// The one-time browser login that authorizes tunnel and DNS changes.
fn require_login() -> Result<(), CliError> {
    let cert = cloudflared_home().join("cert.pem");
    if cert.is_file() {
        return Ok(());
    }
    Err(CliError::Config(format!(
        "Cloudflare is not authorized on this machine ({} is missing).\n\
         Run this once, in a terminal with a browser available:\n\
         \n    cloudflared tunnel login\n\n\
         Pick the zone that will host the worker's hostname.",
        cert.display()
    )))
}

fn require_cloudflared() -> Result<(), CliError> {
    if proc::available("cloudflared") {
        return Ok(());
    }
    Err(CliError::Config(
        "cloudflared is not installed. Install it with: brew install cloudflared".into(),
    ))
}

/// Existing tunnels are reused. `tunnel init` is expected to be re-run — after
/// a hostname change, or just to confirm the setup — and creating a second
/// tunnel with the same name is not what the operator means.
fn ensure_tunnel(name: &str) -> Result<String, CliError> {
    match proc::capture("cloudflared", &["tunnel", "create", name]) {
        Ok(output) => Ok(output),
        Err(error) => {
            let message = error.to_string();
            if message.contains("already exists") {
                Ok(format!("tunnel {name} already exists"))
            } else {
                Err(error)
            }
        }
    }
}

/// The tunnel's UUID.
///
/// `cloudflared tunnel create` writes its credentials to `<uuid>.json`, never
/// `<name>.json`, so the config has to name the file by id or the tunnel
/// refuses to run with "credentials file doesn't exist".
fn tunnel_id(name: &str) -> Result<String, CliError> {
    let listing = proc::capture("cloudflared", &["tunnel", "list", "--output", "json"])?;
    let tunnels: serde_json::Value = serde_json::from_str(&listing)
        .map_err(|error| CliError::Command(format!("cannot read the tunnel list: {error}")))?;
    tunnels
        .as_array()
        .and_then(|tunnels| {
            tunnels
                .iter()
                .find(|tunnel| tunnel.get("name").and_then(serde_json::Value::as_str) == Some(name))
        })
        .and_then(|tunnel| tunnel.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .ok_or_else(|| CliError::Command(format!("no tunnel named {name} in the account")))
}

pub fn init(config: &Config) -> Result<(), CliError> {
    let json = config.json;
    let name = config.tunnel_name.as_str();
    let hostname = config.require_hostname()?;
    require_cloudflared()?;
    require_login()?;

    let created = ensure_tunnel(name)?;
    let routed = proc::capture(
        "cloudflared",
        &["tunnel", "route", "dns", "--overwrite-dns", name, hostname],
    )?;

    // The ingress config is written here rather than passed on the command
    // line so that `cloudflared tunnel run` and a later launchd/systemd unit
    // behave identically.
    let env = worker_env::load(&worker_env::default_path())?;
    let service = env.api_base();
    let id = tunnel_id(name)?;
    let credentials = cloudflared_home().join(format!("{id}.json"));
    if !credentials.is_file() {
        return Err(CliError::Config(format!(
            "tunnel {name} has no credentials file at {}. Re-run `cloudflared tunnel create {name}` on this machine.",
            credentials.display()
        )));
    }
    let config_path = cloudflared_home().join("config.yml");
    let config = format!(
        "# Written by giw tunnel init.\n\
         tunnel: {name}\n\
         credentials-file: {}\n\
         \n\
         ingress:\n\
         \x20 # Only the webhook endpoint is exposed. The build API stays on\n\
         \x20 # localhost: it takes a shared secret, not a signature.\n\
         \x20 - hostname: {hostname}\n\
         \x20   path: ^/webhooks/github$\n\
         \x20   service: {service}\n\
         \x20 - service: http_status:404\n",
        credentials.display()
    );
    std::fs::write(&config_path, config).map_err(|error| {
        CliError::Command(format!("cannot write {}: {error}", config_path.display()))
    })?;

    let machine = serde_json::json!({
        "tunnel": name,
        "hostname": hostname,
        "service": service,
        "config": config_path.to_string_lossy(),
    })
    .to_string();
    let human = format!(
        "{created}\n{routed}\nwrote {}\n\nnext:\n  giw tunnel-up --detach\n  giw webhook-install --repo=OWNER/NAME --webhook-url=https://{hostname}/webhooks/github",
        config_path.display()
    );
    runtime::emit(json, human, &machine)
}

pub fn up(config: &Config) -> Result<(), CliError> {
    let (json, detach) = (config.json, config.detach);
    let name = config.tunnel_name.as_str();
    require_cloudflared()?;
    require_login()?;
    if !detach {
        return proc::inherit("cloudflared", &["tunnel", "run", name], &[]);
    }
    let log_path = cloudflared_home().join(format!("{name}.log"));
    let pid = proc::background("cloudflared", &["tunnel", "run", name], &[], &log_path)?;
    let machine =
        serde_json::json!({ "pid": pid, "tunnel": name, "log": log_path.to_string_lossy() })
            .to_string();
    let human = format!(
        "tunnel {name} started (pid {pid})\n  log  {}",
        log_path.display()
    );
    runtime::emit(json, human, &machine)
}
