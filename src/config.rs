#![forbid(unsafe_code)]

#[path = "../generated/rust/env.rs"]
mod env;

use crate::env_map::{truthy, value, EnvMap};
use crate::error::CliError;

const INDIEBUILD_CONFIG_ENV: &str = "INDIEBUILD_CONFIG";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub api_base: String,
    /// Whether `--api-base` was actually given. Without it the launcher config
    /// decides, because a compiled-in default cannot know which port the
    /// worker was started on.
    pub api_base_explicit: bool,
    pub json: bool,
    pub indiebuild_config: String,
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
    pub fn from_env_map(env_map: &EnvMap) -> Result<Self, CliError> {
        let given_api_base = value(env_map, env::API_BASE);
        let api_base = given_api_base.unwrap_or(env::API_BASE_DEFAULT).to_owned();
        let api_base_explicit = given_api_base.is_some_and(|given| given != env::API_BASE_DEFAULT);
        if api_base.trim().is_empty() {
            return Err(CliError::Config("API base is empty".into()));
        }
        let indiebuild_config = value(env_map, INDIEBUILD_CONFIG_ENV)
            .unwrap_or(".indiebuild.toml")
            .to_owned();
        if indiebuild_config.trim().is_empty() {
            return Err(CliError::Config("IndieBuild config path is empty".into()));
        }
        // `--repo owner/name#31` and `--repo owner/name --pr 31` mean the same
        // thing; an explicit --pr wins so the two can never disagree silently.
        let (repo, repo_pull_request) = match value(env_map, env::REPO) {
            Some(spec) => {
                let (repo, number) = crate::args::parse_repo_spec(spec)?;
                (Some(repo), number)
            }
            None => (None, None),
        };
        let pull_request = match value(env_map, env::PR) {
            Some(raw) => Some(
                raw.parse::<u64>()
                    .map_err(|_| CliError::Usage(format!("{raw} is not a pull request number")))?,
            ),
            None => repo_pull_request,
        };

        Ok(Self {
            api_base,
            api_base_explicit,
            json: truthy(env_map, env::JSON),
            indiebuild_config,
            detach: truthy(env_map, env::DETACH),
            hostname: value(env_map, env::HOSTNAME).map(str::to_owned),
            tunnel_name: value(env_map, env::TUNNEL_NAME)
                .unwrap_or(env::TUNNEL_NAME_DEFAULT)
                .to_owned(),
            repo,
            pull_request,
            webhook_url: value(env_map, env::WEBHOOK_URL).map(str::to_owned),
            profile: value(env_map, env::PROFILE)
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
            CliError::Usage("--hostname is required, e.g. --hostname=ci.example.com".into())
        })
    }

    pub fn require_pull_request(&self) -> Result<u64, CliError> {
        self.pull_request
            .ok_or_else(|| CliError::Usage("--pr=N, or --repo=OWNER/NAME#N, is required".into()))
    }
}

#[cfg(test)]
mod local_ops_tests {
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
        // Unset, and the compiled-in default, must both defer: neither knows
        // which port the worker was actually started on.
        for env_map in [map(&[]), map(&[(env::API_BASE, env::API_BASE_DEFAULT)])] {
            let config = Config::from_env_map(&env_map).expect("valid");
            assert_eq!(
                config.resolve_api_base("http://127.0.0.1:19000"),
                "http://127.0.0.1:19000"
            );
        }
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
        assert!(config
            .require_repo()
            .unwrap_err()
            .to_string()
            .contains("--repo"));
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
    fn a_malformed_repo_or_number_fails_at_parse_time() {
        assert!(Config::from_env_map(&map(&[(env::REPO, "not-a-repo")])).is_err());
        assert!(Config::from_env_map(&map(&[(env::PR, "abc")])).is_err());
    }
}
