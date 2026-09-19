#![forbid(unsafe_code)]

//! `giw webhook install` — point a repository's webhook at this worker.

use std::path::PathBuf;

use crate::config::Config;
use crate::error::CliError;
use crate::{proc, runtime, worker_env};

/// Events the worker acts on. `check_run` carries re-run requests from the
/// pull request UI; `pull_request` is the one that starts CI.
const EVENTS: [&str; 2] = ["pull_request", "check_run"];

/// Where the payload is staged for `gh api --input`.
///
/// It carries the webhook secret, so it is written inside the config
/// directory, read back by `gh`, and removed immediately. Passing it on the
/// command line instead would expose it in the process table.
fn payload_path() -> PathBuf {
    worker_env::default_path()
        .parent()
        .map(|parent| parent.join(".webhook-payload.json"))
        .unwrap_or_else(|| PathBuf::from(".webhook-payload.json"))
}

fn write_private(path: &PathBuf, contents: &str) -> Result<(), CliError> {
    std::fs::write(path, contents)
        .map_err(|error| CliError::Command(format!("cannot write {}: {error}", path.display())))?;
    set_owner_only(path)
}

#[cfg(unix)]
fn set_owner_only(path: &PathBuf) -> Result<(), CliError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| CliError::Command(format!("cannot secure {}: {error}", path.display())))
}

#[cfg(not(unix))]
fn set_owner_only(_path: &PathBuf) -> Result<(), CliError> {
    Ok(())
}

pub fn install(config: &Config) -> Result<(), CliError> {
    let json = config.json;
    let repo = config.require_repo()?;
    let url = config.webhook_url.as_deref();
    if !proc::available("gh") {
        return Err(CliError::Config(
            "gh is not installed. Install it with: brew install gh".into(),
        ));
    }
    let env = worker_env::load(&worker_env::default_path())?;
    let secret = env.require("BUILD_SERVER_GITHUB_WEBHOOK_SECRET")?;
    let url = match url {
        Some(url) => url.to_string(),
        None => {
            let base = env.get("PUBLIC_URL").ok_or_else(|| {
                CliError::Usage(
                    "no --url given and PUBLIC_URL is not set in the worker env file".into(),
                )
            })?;
            format!("{}/webhooks/github", base.trim_end_matches('/'))
        }
    };
    if !url.starts_with("https://") {
        // The secret authenticates the payload, but plaintext would expose
        // every delivery in transit.
        return Err(CliError::Usage(format!(
            "webhook URL must be https, got {url}"
        )));
    }

    let payload = serde_json::json!({
        "name": "web",
        "active": true,
        "events": EVENTS,
        "config": {
            "url": url,
            "content_type": "json",
            "secret": secret,
            "insecure_ssl": "0",
        }
    })
    .to_string();

    let path = payload_path();
    write_private(&path, &payload)?;
    let result = proc::capture(
        "gh",
        &[
            "api",
            "-X",
            "POST",
            &format!("repos/{repo}/hooks"),
            "--input",
            &path.to_string_lossy(),
            "--jq",
            ".id",
        ],
    );
    // Remove the staged secret whether or not gh succeeded.
    let _ = std::fs::remove_file(&path);
    let hook_id = result?;

    let machine =
        serde_json::json!({ "repo": repo, "hookId": hook_id, "url": url, "events": EVENTS })
            .to_string();
    let human = format!(
        "webhook {hook_id} installed on {repo}\n  url     {url}\n  events  {}\n\nGitHub sends a ping immediately; the worker answers it with 200.",
        EVENTS.join(", ")
    );
    runtime::emit(json, human, &machine)
}
