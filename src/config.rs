#![forbid(unsafe_code)]

#[path = "../generated/rust/env.rs"]
mod env;

use crate::args::parse_repo_spec;
use crate::env_map::{truthy, value, EnvMap};
use crate::error::CliError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub api_base: String,
    /// Whether `--api-base` was given. Without it the launcher config decides,
    /// because the contract default cannot know the worker's port.
    pub api_base_explicit: bool,
    pub json: bool,
    pub detach: bool,
    pub hostname: Option<String>,
    pub tunnel_name: String,
    /// `owner/name`, with any `#number` split into `pull_request`.
    pub repo: Option<String>,
    pub pull_request: Option<u64>,
    pub webhook_url: Option<String>,
    pub profile: String,
}

impl Config {
    pub fn from_env_map(env: &EnvMap) -> Result<Self, CliError> {
        let explicit = value(env, env::API_BASE);
        let api_base = explicit.unwrap_or(env::API_BASE_DEFAULT).to_owned();
        // The flag parser materializes declared defaults into the map, so
        // presence alone cannot tell a passed flag from an unset one. A value
        // equal to the declared default is treated as unset, which lets the
        // launcher config decide the port the worker was actually started on.
        let api_base_explicit = explicit.is_some_and(|given| given != env::API_BASE_DEFAULT);
        if api_base.trim().is_empty() {
            return Err(CliError::Config("API base is empty".into()));
        }

        // `--repo owner/name#31` and `--repo owner/name --pr 31` mean the same
        // thing; an explicit --pr wins so the two can never disagree silently.
        let (repo, repo_pull_request) = match value(env, env::REPO) {
            Some(spec) => {
                let (repo, number) = parse_repo_spec(spec)?;
                (Some(repo), number)
            }
            None => (None, None),
        };
        let pull_request = match value(env, env::PR) {
            Some(raw) => Some(
                raw.parse::<u64>()
                    .map_err(|_| CliError::Usage(format!("{raw} is not a pull request number")))?,
            ),
            None => repo_pull_request,
        };

        Ok(Self {
            api_base,
            api_base_explicit,
            json: truthy(env, env::JSON),
            detach: truthy(env, env::DETACH),
            hostname: value(env, env::HOSTNAME).map(str::to_owned),
            tunnel_name: value(env, env::TUNNEL_NAME)
                .unwrap_or(env::TUNNEL_NAME_DEFAULT)
                .to_owned(),
            repo,
            pull_request,
            webhook_url: value(env, env::WEBHOOK_URL).map(str::to_owned),
            profile: value(env, env::PROFILE)
                .unwrap_or(env::PROFILE_DEFAULT)
                .to_owned(),
        })
    }

    /// Where the worker is: an explicit flag wins, else the launcher config.
    pub fn resolve_api_base(&self, from_launcher: &str) -> String {
        if self.api_base_explicit {
            self.api_base.clone()
        } else {
            from_launcher.to_string()
        }
    }

    pub fn require_repo(&self) -> Result<&str, CliError> {
        self.repo
            .as_deref()
            .ok_or_else(|| CliError::Usage("--repo=OWNER/NAME is required".into()))
    }

    pub fn require_hostname(&self) -> Result<&str, CliError> {
        self.hostname.as_deref().ok_or_else(|| {
            CliError::Usage(
                "--hostname is required, e.g. --hostname=ci-worker.example.com".into(),
            )
        })
    }

    pub fn require_pull_request(&self) -> Result<u64, CliError> {
        self.pull_request
            .ok_or_else(|| CliError::Usage("--pr=N, or --repo=OWNER/NAME#N, is required".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> EnvMap {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn an_explicit_api_base_beats_the_launcher_config() {
        let explicit =
            Config::from_env_map(&map(&[(env::API_BASE, "http://127.0.0.1:9999")])).expect("valid");
        assert_eq!(
            explicit.resolve_api_base("http://127.0.0.1:18095"),
            "http://127.0.0.1:9999"
        );
        // Without the flag the contract default must not win: it cannot know
        // which port the worker was launched on.
        let implicit = Config::from_env_map(&map(&[])).expect("valid");
        assert_eq!(
            implicit.resolve_api_base("http://127.0.0.1:19000"),
            "http://127.0.0.1:19000"
        );
        // The parser fills declared defaults in, so that must read as unset
        // too, or the launcher config could never take effect.
        let defaulted =
            Config::from_env_map(&map(&[(env::API_BASE, env::API_BASE_DEFAULT)])).expect("valid");
        assert_eq!(
            defaulted.resolve_api_base("http://127.0.0.1:19000"),
            "http://127.0.0.1:19000"
        );
    }

    #[test]
    fn a_repo_spec_supplies_the_pull_request_number() {
        let config = Config::from_env_map(&map(&[(env::REPO, "owner/name#31")])).expect("valid");
        assert_eq!(config.repo.as_deref(), Some("owner/name"));
        assert_eq!(config.pull_request, Some(31));
    }

    #[test]
    fn an_explicit_pr_flag_wins_over_the_spec_suffix() {
        let config = Config::from_env_map(&map(&[(env::REPO, "owner/name#31"), (env::PR, "42")]))
            .expect("valid");
        assert_eq!(config.pull_request, Some(42));
    }

    #[test]
    fn defaults_come_from_the_flags_contract() {
        let config = Config::from_env_map(&map(&[])).expect("valid");
        assert_eq!(config.tunnel_name, env::TUNNEL_NAME_DEFAULT);
        assert_eq!(config.profile, env::PROFILE_DEFAULT);
        assert!(!config.detach);
    }

    #[test]
    fn missing_required_values_name_the_flag() {
        let config = Config::from_env_map(&map(&[])).expect("valid");
        assert!(config.require_repo().unwrap_err().to_string().contains("--repo"));
        assert!(config
            .require_hostname()
            .unwrap_err()
            .to_string()
            .contains("--hostname"));
        assert!(config
            .require_pull_request()
            .unwrap_err()
            .to_string()
            .contains("--pr"));
    }

    #[test]
    fn a_malformed_repo_fails_at_parse_time() {
        assert!(Config::from_env_map(&map(&[(env::REPO, "not-a-repo")])).is_err());
        assert!(Config::from_env_map(&map(&[(env::PR, "abc")])).is_err());
    }
}
