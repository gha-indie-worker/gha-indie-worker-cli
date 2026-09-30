#![forbid(unsafe_code)]

//! `giw webhook-install` — point a repository's webhook at this worker.
//!
//! Installation converges. Running it twice leaves one hook, because a second
//! hook on the same URL means every GitHub event is delivered twice and the
//! worker builds and reports the same commit twice. Re-running is also how the
//! webhook secret is rotated.

use crate::config::Config;
use crate::error::CliError;
use crate::{proc, runtime, worker_env};

/// Events the worker acts on. `check_run` carries re-run requests from the
/// pull request UI; `pull_request` is the one that starts CI.
const EVENTS: [&str; 2] = ["pull_request", "check_run"];

/// What installing should do, given the hooks a repository already has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallPlan {
    /// No hook delivers to this URL yet.
    Create,
    /// Exactly one does: update it in place, which also rotates its secret.
    Update(u64),
    /// More than one does. Picking one would leave the others delivering
    /// duplicates, and deleting them is not this command's call to make.
    Conflict(Vec<u64>),
}

/// Decide from the repository's hook listing, without touching the network.
///
/// URLs are compared after trimming a trailing slash, since GitHub stores what
/// it was given and `…/github` and `…/github/` are the same endpoint.
pub fn plan_install(hooks: &serde_json::Value, url: &str) -> InstallPlan {
    let wanted = url.trim_end_matches('/');
    let matching: Vec<u64> = hooks
        .as_array()
        .map(|hooks| {
            hooks
                .iter()
                .filter(|hook| {
                    hook.pointer("/config/url")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|existing| existing.trim_end_matches('/') == wanted)
                })
                .filter_map(|hook| hook.get("id").and_then(serde_json::Value::as_u64))
                .collect()
        })
        .unwrap_or_default();
    match matching.as_slice() {
        [] => InstallPlan::Create,
        [only] => InstallPlan::Update(*only),
        _ => InstallPlan::Conflict(matching),
    }
}

pub fn install(config: &Config) -> Result<(), CliError> {
    let json = config.json;
    let repo = config.require_repo()?;
    if !proc::available("gh") {
        return Err(CliError::Config(
            "gh is not installed. Install it with: brew install gh".into(),
        ));
    }
    let env = worker_env::load(&worker_env::default_path())?;
    let secret = env.require("BUILD_SERVER_GITHUB_WEBHOOK_SECRET")?;
    let url = match config.webhook_url.as_deref() {
        Some(url) => url.to_string(),
        None => {
            let base = env.get("PUBLIC_URL").ok_or_else(|| {
                CliError::Usage(
                    "no --webhook-url given and PUBLIC_URL is not set in the worker env file"
                        .into(),
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

    let existing = proc::capture(
        "gh",
        &[
            "api",
            "--paginate",
            "--slurp",
            &format!("repos/{repo}/hooks?per_page=100"),
        ],
    )?;
    // `--slurp` wraps each page in an outer array; flatten to one listing.
    let pages: serde_json::Value = serde_json::from_str(&existing)
        .map_err(|error| CliError::Command(format!("cannot read the hook listing: {error}")))?;
    let hooks = serde_json::Value::Array(
        pages
            .as_array()
            .map(|pages| {
                pages
                    .iter()
                    .flat_map(|page| page.as_array().cloned().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default(),
    );

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

    let (method, path, action) = match plan_install(&hooks, &url) {
        InstallPlan::Create => ("POST", format!("repos/{repo}/hooks"), "installed"),
        InstallPlan::Update(id) => ("PATCH", format!("repos/{repo}/hooks/{id}"), "updated"),
        InstallPlan::Conflict(ids) => {
            let listed = ids
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(CliError::Command(format!(
                "{repo} already has {} hooks delivering to {url} (ids {listed}); every event is being delivered more than once. Delete the extras, then re-run.",
                ids.len()
            )));
        }
    };

    // The payload carries the webhook secret, so it goes to `gh` on stdin. It
    // is never in argv, where the process table would show it, and never in a
    // file, which would need a path, permissions and cleanup to be right under
    // local concurrency.
    let hook_id = proc::capture_with_stdin(
        "gh",
        &["api", "-X", method, &path, "--input", "-", "--jq", ".id"],
        payload.as_bytes(),
    )?;

    let machine = serde_json::json!({
        "repo": repo,
        "hookId": hook_id,
        "url": url,
        "events": EVENTS,
        "action": action,
    })
    .to_string();
    let human = format!(
        "webhook {hook_id} {action} on {repo}\n  url     {url}\n  events  {}\n\nGitHub sends a ping on creation; the worker answers it with 200.",
        EVENTS.join(", ")
    );
    runtime::emit(json, human, &machine)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const URL: &str = "https://ci.example.com/webhooks/github";

    #[test]
    fn a_repository_with_no_matching_hook_gets_one() {
        assert_eq!(plan_install(&json!([]), URL), InstallPlan::Create);
        let unrelated = json!([{ "id": 1, "config": { "url": "https://other.example.com/hook" } }]);
        assert_eq!(plan_install(&unrelated, URL), InstallPlan::Create);
    }

    #[test]
    fn installing_twice_converges_on_one_hook() {
        // First run creates. The second sees that hook and updates it in place
        // rather than appending a duplicate that would double every delivery.
        let after_first_run = json!([{ "id": 681693899u64, "config": { "url": URL } }]);
        assert_eq!(
            plan_install(&after_first_run, URL),
            InstallPlan::Update(681693899)
        );
    }

    #[test]
    fn a_trailing_slash_is_the_same_endpoint() {
        let stored = json!([{ "id": 7, "config": { "url": format!("{URL}/") } }]);
        assert_eq!(plan_install(&stored, URL), InstallPlan::Update(7));
    }

    #[test]
    fn duplicates_are_reported_rather_than_added_to() {
        let duplicated = json!([
            { "id": 1, "config": { "url": URL } },
            { "id": 2, "config": { "url": "https://other.example.com/hook" } },
            { "id": 3, "config": { "url": URL } }
        ]);
        assert_eq!(
            plan_install(&duplicated, URL),
            InstallPlan::Conflict(vec![1, 3])
        );
    }

    #[test]
    fn a_malformed_listing_does_not_invent_a_match() {
        assert_eq!(
            plan_install(&json!({ "message": "Not Found" }), URL),
            InstallPlan::Create
        );
        let missing_fields = json!([{ "id": 1 }, { "config": { "url": URL } }]);
        // The second entry matches but has no id to update, so it cannot be
        // treated as the hook to converge on.
        assert_eq!(plan_install(&missing_fields, URL), InstallPlan::Create);
    }

    #[test]
    fn the_secret_never_reaches_a_file_or_argv() {
        // Source-level guard: this command must hand the payload to `gh` on
        // stdin. A regression to a temp file or a field flag would reintroduce
        // the exposure the stdin path exists to close.
        const SOURCE: &str = include_str!("webhook.rs");
        let production = SOURCE.split("#[cfg(test)]").next().unwrap_or(SOURCE);
        assert!(production.contains("capture_with_stdin"));
        assert!(!production.contains("fs::write"));
        assert!(!production.contains("set_permissions"));
    }
}
