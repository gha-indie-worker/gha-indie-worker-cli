#![forbid(unsafe_code)]

use crate::error::CliError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Health,
    Status,
    IndieBuildValidate,
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

/// Parse an exact repository identifier or pasted GitHub pull-request URL.
///
/// Accepted: `owner/repo`, `owner/repo#31`,
/// `https://github.com/owner/repo` and
/// `https://github.com/owner/repo/pull/31`.
/// Queries, fragments, alternate hosts and unexpected URL routes fail closed.
fn parse_pr_number(raw: &str) -> Result<u64, CliError> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CliError::Usage(format!(
            "{raw} is not a pull request number"
        )));
    }
    match raw.parse::<u64>() {
        Ok(number) if number > 0 => Ok(number),
        Ok(_) => Err(CliError::Usage("pull request numbers start at 1".into())),
        Err(_) => Err(CliError::Usage(format!(
            "{raw} is not a pull request number"
        ))),
    }
}

pub fn parse_repo_spec(spec: &str) -> Result<(String, Option<u64>), CliError> {
    let value = spec.trim();
    let invalid = || {
        CliError::Usage(format!(
            "expected owner/name, owner/name#N or https://github.com/owner/name/pull/N, got {spec:?}"
        ))
    };

    let (repo, number) = if let Some(path) = value.strip_prefix("https://github.com/") {
        // Permit one trailing slash, but not arbitrary nested GitHub routes.
        let path = path.strip_suffix('/').unwrap_or(path);
        match path.split('/').collect::<Vec<_>>().as_slice() {
            [owner, name] => (format!("{owner}/{name}"), None),
            [owner, name, "pull", raw] => (format!("{owner}/{name}"), Some(parse_pr_number(raw)?)),
            _ => return Err(invalid()),
        }
    } else {
        match value.split_once('#') {
            Some((repo, raw)) => (repo.to_string(), Some(parse_pr_number(raw)?)),
            None => (value.to_string(), None),
        }
    };

    let (owner, name) = repo.split_once('/').ok_or_else(invalid)?;
    let valid_component = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    };
    if !valid_component(owner) || !valid_component(name) {
        return Err(invalid());
    }
    Ok((repo, number))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub command: Command,
    pub api_base: Option<String>,
    pub json: bool,
}

pub fn parse<I>(args: I) -> Result<Invocation, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut command = Command::Help;
    let mut api_base = None;
    let mut json = false;
    let mut items = args.into_iter().peekable();
    if let Some(first) = items.peek() {
        match first.as_str() {
            "health" => {
                command = Command::Health;
                items.next();
            }
            "status" => {
                command = Command::Status;
                items.next();
            }
            "indiebuild-validate" => {
                command = Command::IndieBuildValidate;
                items.next();
            }
            "-h" | "--help" | "help" => {
                command = Command::Help;
                items.next();
            }
            other if !other.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown command {other}")));
            }
            _ => {}
        }
    }
    for arg in items {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => command = Command::Help,
            flag if flag.starts_with("--api-base=") => {
                api_base = Some(flag.trim_start_matches("--api-base=").to_string());
            }
            other => return Err(CliError::Usage(format!("unknown flag {other}"))),
        }
    }
    Ok(Invocation {
        command,
        api_base,
        json,
    })
}

pub fn help_text() -> &'static str {
    "giw — GHA Indie Worker CLI\n\
\n\
Run pull request CI on this machine instead of GitHub-hosted runners.\n\
\n\
Commands:\n\
  health                    worker liveness\n\
  status                    worker readiness and container runtime\n\
  indiebuild-validate       validate .indiebuild.toml and print it normalized\n\
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
  giw tunnel-init --hostname=ci.example.com\n\
  giw worker-up --detach && giw tunnel-up --detach\n\
  giw webhook-install --repo=OWNER/NAME\n\
\n\
Configuration is read from ~/.config/gha-indie-worker/worker.env,\n\
or GHAIW_ENV_FILE.\n"
}

#[cfg(test)]
mod repo_spec_tests {
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
    fn pasted_github_urls_preserve_repository_and_pr_identity() {
        for (spec, expected_number) in [
            (
                "https://github.com/gha-indie-worker/gha-indie-worker.rs/pull/123",
                Some(123),
            ),
            (
                "https://github.com/gha-indie-worker/gha-indie-worker.rs/pull/123/",
                Some(123),
            ),
            (
                "https://github.com/gha-indie-worker/gha-indie-worker.rs",
                None,
            ),
            (
                "https://github.com/gha-indie-worker/gha-indie-worker.rs/",
                None,
            ),
        ] {
            assert_eq!(
                parse_repo_spec(spec).unwrap(),
                (
                    "gha-indie-worker/gha-indie-worker.rs".to_string(),
                    expected_number
                )
            );
        }
    }

    #[test]
    fn malformed_specs_are_rejected_rather_than_guessed() {
        for bad in [
            "owner",
            "a/b/c",
            "/name",
            "owner/",
            "owner/name#x",
            "owner/name#0",
            "owner/name#-1",
            "owner/name#1#2",
            "owner/na me",
            "owner/..",
            "owner/name/other",
            "https://github.com/owner/repo/issues/31",
            "https://github.com/owner/repo/pull/0",
            "https://github.com/owner/repo/pull/31?tab=files",
            "https://github.com/owner/repo/pull/31//",
            "https://github.com.evil.test/owner/repo/pull/31",
        ] {
            assert!(parse_repo_spec(bad).is_err(), "{bad} must not parse");
        }
    }
}
