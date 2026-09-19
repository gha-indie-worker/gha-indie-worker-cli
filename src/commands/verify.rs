#![forbid(unsafe_code)]

//! `ghaiw verify OWNER/REPO#N` — verify one open pull request now.
//!
//! The same path a webhook delivery takes, driven by hand: resolve the head
//! commit, submit it, wait, and let the worker report the verdict to GitHub.
//! Useful for pull requests that were opened before the webhook existed.

use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::error::CliError;
use crate::{http_client, proc, worker_env};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(45 * 60);

fn terminal(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "failed" | "error" | "cancelled" | "timeout"
    )
}

pub fn run(config: &Config) -> Result<(), CliError> {
    let json = config.json;
    let repo = config.require_repo()?;
    let pull_request = config.require_pull_request()?;
    let profile = config.profile.as_str();
    if !proc::available("gh") {
        return Err(CliError::Config(
            "gh is not installed. Install it with: brew install gh".into(),
        ));
    }
    let env = worker_env::load(&worker_env::default_path())?;
    let base = config.resolve_api_base(&env.api_base());
    let auth = env.require("BUILD_SERVER_AUTH_SECRET")?.to_string();

    // One call for everything the decision needs, so the head SHA and the fork
    // check describe the same state of the pull request.
    let details = proc::capture(
        "gh",
        &[
            "api",
            &format!("repos/{repo}/pulls/{pull_request}"),
            "--jq",
            "[.head.sha, (.head.repo.full_name // \"\"), .state, (.draft|tostring)] | @tsv",
        ],
    )?;
    let fields: Vec<&str> = details.split('\t').collect();
    let [head_sha, head_repo, state, draft] = fields.as_slice() else {
        return Err(CliError::Command(format!(
            "unexpected pull request response: {details}"
        )));
    };
    if *state != "open" {
        return Err(CliError::Usage(format!(
            "{repo}#{pull_request} is {state}, not open"
        )));
    }
    if !head_repo.eq_ignore_ascii_case(repo) {
        // Same rule the webhook path enforces: a fork's head is
        // attacker-controlled code and does not run on this machine.
        return Err(CliError::Usage(format!(
            "{repo}#{pull_request} comes from the fork {head_repo}, which this worker will not build"
        )));
    }
    if *draft == "true" {
        eprintln!("note: {repo}#{pull_request} is a draft; verifying it anyway");
    }

    let request = serde_json::json!({
        "jobKind": "run-profile",
        "repoUrl": format!("https://github.com/{repo}"),
        "gitRef": head_sha,
        "profile": profile,
    })
    .to_string();
    let submitted = http_client::request(
        &base,
        "POST",
        "/builds",
        &[("x-server-auth", auth.as_str())],
        Some(&request),
    )?;
    if !submitted.is_success() {
        return Err(CliError::Command(format!(
            "worker refused the build ({}): {}",
            submitted.status, submitted.body
        )));
    }
    let job_id = http_client::json_field(&submitted.body, "id")
        .ok_or_else(|| CliError::Command(format!("no job id in response: {}", submitted.body)))?;
    if !json {
        println!("submitted {job_id} for {repo}#{pull_request} at {head_sha}");
    }

    let started = Instant::now();
    let mut status = "queued".to_string();
    while started.elapsed() < MAX_WAIT {
        sleep(POLL_INTERVAL);
        let response = http_client::request(
            &base,
            "GET",
            &format!("/builds/{job_id}"),
            &[("x-server-auth", auth.as_str())],
            None,
        )?;
        if let Some(current) = http_client::json_field(&response.body, "status") {
            if current != status && !json {
                println!("  {current}");
            }
            status = current;
            if terminal(&status) {
                break;
            }
        }
    }

    let succeeded = status == "succeeded";
    if json {
        println!(
            "{}",
            serde_json::json!({
                "repo": repo,
                "pullRequest": pull_request,
                "headSha": head_sha,
                "jobId": job_id,
                "status": status,
                "succeeded": succeeded,
            })
        );
    } else if succeeded {
        println!("{repo}#{pull_request} verified; the commit status is reported on GitHub");
    } else {
        println!("{repo}#{pull_request} did not pass ({status})");
        println!("  logs: ghaiw status, or {base}/builds/{job_id}/logs");
    }
    if succeeded {
        Ok(())
    } else {
        Err(CliError::Command(format!("verification {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_finished_states_stop_the_poll_loop() {
        for done in ["succeeded", "failed", "error", "cancelled", "timeout"] {
            assert!(terminal(done), "{done} should end the wait");
        }
        for busy in ["queued", "running", "starting"] {
            assert!(!terminal(busy), "{busy} should keep waiting");
        }
    }
}
