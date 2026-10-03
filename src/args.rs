#![forbid(unsafe_code)]

use crate::error::CliError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Health,
    Status,
    /// Start the build worker on this machine.
    WorkerUp,
    /// Create the named tunnel and its DNS route.
    TunnelInit,
    /// Run the named tunnel.
    TunnelUp,
    /// Point a repository's webhook at this worker.
    WebhookInstall,
    /// Verify one open pull request and report the result.
    Verify,
}

/// Split `owner/repo` or `owner/repo#31` into its parts.
///
/// Accepting the `#number` form means a pull request can be pasted exactly as
/// it is written down, without splitting it across two flags by hand.
pub fn parse_repo_spec(spec: &str) -> Result<(String, Option<u64>), CliError> {
    let (repo, number) = match spec.split_once('#') {
        Some((repo, number)) => {
            let parsed = number
                .parse::<u64>()
                .map_err(|_| CliError::Usage(format!("{number} is not a pull request number")))?;
            if parsed == 0 {
                return Err(CliError::Usage("pull request numbers start at 1".into()));
            }
            (repo, Some(parsed))
        }
        None => (spec, None),
    };
    let repo = repo.trim();
    if repo.matches('/').count() != 1 || repo.starts_with('/') || repo.ends_with('/') {
        return Err(CliError::Usage(format!(
            "expected owner/name, got {spec:?}"
        )));
    }
    Ok((repo.to_string(), number))
}

pub fn help_text() -> &'static str {
    "ghaiw — GHA Indie Worker CLI\n\
\n\
Run pull request CI on this machine instead of GitHub-hosted runners.\n\
\n\
Commands:\n\
  health                    worker liveness\n\
  status                    worker readiness and queue counters\n\
  worker-up [--detach]      start the build worker\n\
  tunnel-init --hostname=H  create the named tunnel and its DNS route\n\
  tunnel-up [--detach]      run the tunnel\n\
  webhook-install --repo=O/R [--webhook-url=U]\n\
                            point a repository's webhook at this worker\n\
  verify --repo=O/R#N [--profile=P]\n\
                            verify one open pull request and report it\n\
\n\
Flags:\n\
  --api-base=URL            worker base URL (default: from worker.env)\n\
  --tunnel-name=NAME        tunnel name (default: ci-worker)\n\
  --json                    machine-readable output\n\
\n\
Setup, once:\n\
  cloudflared tunnel login\n\
  ghaiw tunnel-init --hostname=ci-worker.example.com\n\
  ghaiw worker-up --detach && ghaiw tunnel-up --detach\n\
  ghaiw webhook-install --repo=OWNER/NAME\n\
\n\
Configuration is read from ~/.config/gha-indie-worker/worker.env,\n\
or GHAIW_ENV_FILE.\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repo_spec_may_carry_the_pull_request_number() {
        assert_eq!(
            parse_repo_spec("gha-indie-worker/api.rs#31").unwrap(),
            ("gha-indie-worker/api.rs".to_string(), Some(31))
        );
        assert_eq!(
            parse_repo_spec("owner/name").unwrap(),
            ("owner/name".to_string(), None)
        );
    }

    #[test]
    fn malformed_specs_are_rejected_rather_than_guessed() {
        for bad in ["owner", "a/b/c", "/name", "owner/", "owner/name#x", "owner/name#0"] {
            assert!(parse_repo_spec(bad).is_err(), "{bad} must not parse");
        }
    }
}
