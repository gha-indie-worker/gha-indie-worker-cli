#![forbid(unsafe_code)]

//! The worker's launch configuration file.
//!
//! One `KEY=VALUE` per line, which is the form the worker already reads from
//! its environment. Keeping it a file rather than a shell snippet means the
//! CLI can read individual settings — the port to talk to, the webhook secret
//! to register with GitHub — without executing anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::CliError;

/// Where the launcher config lives unless `GHAIW_ENV_FILE` says otherwise.
pub const DEFAULT_RELATIVE_PATH: &str = ".config/gha-indie-worker/worker.env";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkerEnv {
    values: BTreeMap<String, String>,
}

impl WorkerEnv {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn require(&self, key: &str) -> Result<&str, CliError> {
        self.get(key)
            .ok_or_else(|| CliError::Config(format!("{key} is not set in the worker env file")))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// The worker's local base URL, from HOST and PORT.
    pub fn api_base(&self) -> String {
        let host = self.get("HOST").unwrap_or("127.0.0.1");
        let port = self.get("PORT").unwrap_or("18095");
        format!("http://{host}:{port}")
    }
}

/// Parse the file's contents.
///
/// Values are taken literally: no shell expansion, no quote stripping beyond a
/// single matching pair, because a webhook secret may legitimately contain
/// characters a shell would treat as syntax.
pub fn parse(contents: &str) -> WorkerEnv {
    let mut values = BTreeMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = match (value.chars().next(), value.chars().last(), value.len()) {
            (Some('"'), Some('"'), len) if len >= 2 => &value[1..len - 1],
            (Some('\''), Some('\''), len) if len >= 2 => &value[1..len - 1],
            _ => value,
        };
        values.insert(key.to_string(), value.to_string());
    }
    WorkerEnv { values }
}

pub fn default_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("GHAIW_ENV_FILE") {
        return PathBuf::from(explicit);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(DEFAULT_RELATIVE_PATH)
}

pub fn load(path: &Path) -> Result<WorkerEnv, CliError> {
    let contents = std::fs::read_to_string(path).map_err(|error| {
        CliError::Config(format!(
            "cannot read worker env file {}: {error}",
            path.display()
        ))
    })?;
    Ok(parse(&contents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let env = parse("# a comment\n\n  \nPORT=18095\n");
        assert_eq!(env.get("PORT"), Some("18095"));
        assert_eq!(env.iter().count(), 1);
    }

    #[test]
    fn values_keep_characters_a_shell_would_eat() {
        // Secrets and allowlists legitimately contain these.
        let env = parse(
            "BUILD_SERVER_GITHUB_WEBHOOK_SECRET=a$b&c|d(e)\nPREFIXES=https://a/,https://b/\n",
        );
        assert_eq!(
            env.get("BUILD_SERVER_GITHUB_WEBHOOK_SECRET"),
            Some("a$b&c|d(e)")
        );
        assert_eq!(env.get("PREFIXES"), Some("https://a/,https://b/"));
    }

    #[test]
    fn a_value_may_contain_equals_signs() {
        let env = parse("NAME=indiebuild / local-ci\nB64=aGk=\n");
        assert_eq!(env.get("NAME"), Some("indiebuild / local-ci"));
        assert_eq!(env.get("B64"), Some("aGk="));
    }

    #[test]
    fn one_matching_quote_pair_is_stripped() {
        let env = parse("A=\"quoted\"\nB='single'\nC=\"unbalanced\n");
        assert_eq!(env.get("A"), Some("quoted"));
        assert_eq!(env.get("B"), Some("single"));
        assert_eq!(env.get("C"), Some("\"unbalanced"));
    }

    #[test]
    fn api_base_falls_back_to_the_documented_defaults() {
        assert_eq!(parse("").api_base(), "http://127.0.0.1:18095");
        assert_eq!(
            parse("HOST=0.0.0.0\nPORT=9000").api_base(),
            "http://0.0.0.0:9000"
        );
    }

    #[test]
    fn require_names_the_missing_key() {
        let error = parse("").require("BUILD_SERVER_AUTH_SECRET").unwrap_err();
        assert!(
            error.to_string().contains("BUILD_SERVER_AUTH_SECRET"),
            "{error}"
        );
    }
}
