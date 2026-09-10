#![forbid(unsafe_code)]

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path},
};

use serde::{Deserialize, Serialize};

use crate::error::CliError;

pub const CONTRACT_VERSION: &str = "gha-indie-worker.indiebuild/v1";
pub const DEFAULT_CONFIG_PATH: &str = ".indiebuild.toml";
const MAX_CONFIG_BYTES: u64 = 256 * 1024;
const MAX_TARGETS: usize = 64;
const MAX_LIST_ITEMS: usize = 128;
const MAX_NAME_BYTES: usize = 128;
const MAX_TIMEOUT_SECONDS: u32 = 86_400;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryRole {
    Client,
    Server,
    Mixed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetRole {
    Client,
    Server,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub name: String,
    pub role: TargetRole,
    pub path: String,
    pub profile: String,
    pub platform: Platform,
    pub artifacts: Vec<String>,
    pub cache_paths: Vec<String>,
    pub env: Vec<String>,
    pub secret_env: Vec<String>,
    pub allow_network: bool,
    pub allow_push: bool,
    pub allow_deploy: bool,
    pub timeout_seconds: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IndieBuildConfig {
    pub schema_version: String,
    pub repository_role: RepositoryRole,
    pub default_target: String,
    pub targets: Vec<Target>,
}

impl IndieBuildConfig {
    pub fn parse(input: &str) -> Result<Self, CliError> {
        let config: Self = toml::from_str(input)
            .map_err(|error| CliError::Config(format!("invalid {DEFAULT_CONFIG_PATH}: {error}")))?;
        config.validate()?;
        Ok(config)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, CliError> {
        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|error| {
            CliError::Config(format!("cannot read {}: {error}", path.display()))
        })?;
        if !metadata.is_file() {
            return Err(CliError::Config(format!(
                "{} is not a regular file",
                path.display()
            )));
        }
        if metadata.len() > MAX_CONFIG_BYTES {
            return Err(CliError::Config(format!(
                "{} exceeds the {MAX_CONFIG_BYTES}-byte IndieBuild config limit",
                path.display()
            )));
        }
        let input = fs::read_to_string(path).map_err(|error| {
            CliError::Config(format!(
                "cannot read {} as UTF-8 text: {error}",
                path.display()
            ))
        })?;
        Self::parse(&input)
    }

    pub fn validate(&self) -> Result<(), CliError> {
        if self.schema_version != CONTRACT_VERSION {
            return Err(config_error(format!(
                "schema_version must equal {CONTRACT_VERSION:?}"
            )));
        }
        validate_token("default_target", &self.default_target)?;
        if self.targets.is_empty() || self.targets.len() > MAX_TARGETS {
            return Err(config_error(format!(
                "targets must contain 1-{MAX_TARGETS} entries"
            )));
        }

        let mut names = HashSet::with_capacity(self.targets.len());
        let mut client_targets = 0usize;
        let mut server_targets = 0usize;
        for target in &self.targets {
            validate_target(target)?;
            if !names.insert(target.name.as_str()) {
                return Err(config_error(format!(
                    "duplicate target name {:?}",
                    target.name
                )));
            }
            match target.role {
                TargetRole::Client => client_targets += 1,
                TargetRole::Server => server_targets += 1,
            }
        }
        if !names.contains(self.default_target.as_str()) {
            return Err(config_error(format!(
                "default_target {:?} does not name a declared target",
                self.default_target
            )));
        }

        match self.repository_role {
            RepositoryRole::Client if server_targets != 0 => {
                return Err(config_error(
                    "repository_role=client cannot contain server targets".to_owned(),
                ));
            }
            RepositoryRole::Server if client_targets != 0 => {
                return Err(config_error(
                    "repository_role=server cannot contain client targets".to_owned(),
                ));
            }
            RepositoryRole::Mixed if client_targets == 0 || server_targets == 0 => {
                return Err(config_error(
                    "repository_role=mixed requires at least one client and one server target"
                        .to_owned(),
                ));
            }
            _ => {}
        }
        Ok(())
    }

    pub fn default(&self) -> &Target {
        self.targets
            .iter()
            .find(|target| target.name == self.default_target)
            .expect("validated IndieBuild config must contain default_target")
    }

    pub fn target(&self, name: &str) -> Result<&Target, CliError> {
        self.targets
            .iter()
            .find(|target| target.name == name)
            .ok_or_else(|| config_error(format!("unknown IndieBuild target {name:?}")))
    }

    pub fn normalized_json(&self) -> Result<String, CliError> {
        serde_json::to_string_pretty(self)
            .map_err(|error| config_error(format!("cannot encode normalized config: {error}")))
    }
}

fn validate_target(target: &Target) -> Result<(), CliError> {
    validate_token("target.name", &target.name)?;
    validate_token("target.profile", &target.profile)?;
    validate_repo_relative_path("target.path", &target.path)?;
    validate_path_list("target.artifacts", &target.artifacts)?;
    validate_path_list("target.cache_paths", &target.cache_paths)?;
    validate_env_list("target.env", &target.env)?;
    validate_env_list("target.secret_env", &target.secret_env)?;

    let plain = target
        .env
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if let Some(duplicate) = target
        .secret_env
        .iter()
        .map(String::as_str)
        .find(|name| plain.contains(name))
    {
        return Err(config_error(format!(
            "environment name {duplicate:?} cannot be declared in both env and secret_env"
        )));
    }

    if target.role == TargetRole::Client && (target.allow_push || target.allow_deploy) {
        return Err(config_error(format!(
            "client target {:?} cannot enable image push or deployment",
            target.name
        )));
    }
    if target.timeout_seconds == 0 || target.timeout_seconds > MAX_TIMEOUT_SECONDS {
        return Err(config_error(format!(
            "target {:?} timeout_seconds must be 1-{MAX_TIMEOUT_SECONDS}",
            target.name
        )));
    }
    Ok(())
}

fn validate_token(label: &str, value: &str) -> Result<(), CliError> {
    if value.is_empty() || value.len() > MAX_NAME_BYTES {
        return Err(config_error(format!(
            "{label} must be 1-{MAX_NAME_BYTES} bytes"
        )));
    }
    if value.chars().any(char::is_control) || value.chars().any(char::is_whitespace) {
        return Err(config_error(format!(
            "{label} cannot contain whitespace or control characters"
        )));
    }
    Ok(())
}

fn validate_path_list(label: &str, values: &[String]) -> Result<(), CliError> {
    if values.len() > MAX_LIST_ITEMS {
        return Err(config_error(format!(
            "{label} can contain at most {MAX_LIST_ITEMS} entries"
        )));
    }
    let mut unique = HashSet::with_capacity(values.len());
    for value in values {
        validate_repo_relative_path(label, value)?;
        if !unique.insert(value.as_str()) {
            return Err(config_error(format!("duplicate {label} entry {value:?}")));
        }
    }
    Ok(())
}

fn validate_repo_relative_path(label: &str, value: &str) -> Result<(), CliError> {
    if value.trim().is_empty() || value.len() > 240 {
        return Err(config_error(format!(
            "{label} must be a non-empty repository-relative path of at most 240 bytes"
        )));
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(config_error(format!(
            "{label} must be relative to the repository root"
        )));
    }
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part
                    .to_str()
                    .ok_or_else(|| config_error(format!("{label} must be valid UTF-8")))?;
                if part
                    .chars()
                    .any(|ch| matches!(ch, ',' | '=' | ':' | '\0') || ch.is_control())
                {
                    return Err(config_error(format!(
                        "{label} contains unsupported path characters"
                    )));
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(config_error(format!(
                    "{label} must stay inside the repository root"
                )));
            }
        }
    }
    Ok(())
}

fn validate_env_list(label: &str, values: &[String]) -> Result<(), CliError> {
    if values.len() > MAX_LIST_ITEMS {
        return Err(config_error(format!(
            "{label} can contain at most {MAX_LIST_ITEMS} entries"
        )));
    }
    let mut unique = HashSet::with_capacity(values.len());
    for value in values {
        if value.is_empty() || value.len() > MAX_NAME_BYTES {
            return Err(config_error(format!(
                "{label} names must be 1-{MAX_NAME_BYTES} bytes"
            )));
        }
        let mut chars = value.chars();
        let first = chars.next().expect("checked non-empty");
        if !(first.is_ascii_alphabetic() || first == '_')
            || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Err(config_error(format!(
                "{label} entry {value:?} is not a valid environment variable name"
            )));
        }
        if !unique.insert(value.as_str()) {
            return Err(config_error(format!("duplicate {label} entry {value:?}")));
        }
    }
    Ok(())
}

fn config_error(message: String) -> CliError {
    CliError::Config(format!("{DEFAULT_CONFIG_PATH}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: &str = r#"
schema_version = "gha-indie-worker.indiebuild/v1"
repository_role = "server"
default_target = "server"

[[targets]]
name = "server"
role = "server"
path = "."
profile = "rust-verify"
platform = "linux"
artifacts = []
cache_paths = []
env = ["RUST_LOG"]
secret_env = []
allow_network = true
allow_push = false
allow_deploy = false
timeout_seconds = 1800
"#;

    #[test]
    fn accepts_strict_server_contract() {
        let config = IndieBuildConfig::parse(SERVER).expect("valid config");
        assert_eq!(config.repository_role, RepositoryRole::Server);
        assert_eq!(config.default().profile, "rust-verify");
    }

    #[test]
    fn rejects_unknown_fields() {
        let input = SERVER.replace("default_target", "unexpected");
        assert!(IndieBuildConfig::parse(&input).is_err());
    }

    #[test]
    fn mixed_requires_both_roles() {
        let input = SERVER.replace(
            "repository_role = \"server\"",
            "repository_role = \"mixed\"",
        );
        assert!(IndieBuildConfig::parse(&input).is_err());
    }

    #[test]
    fn client_target_cannot_push_or_deploy() {
        let input = SERVER
            .replace(
                "repository_role = \"server\"",
                "repository_role = \"client\"",
            )
            .replace("role = \"server\"", "role = \"client\"")
            .replace("allow_push = false", "allow_push = true");
        assert!(IndieBuildConfig::parse(&input).is_err());
    }

    #[test]
    fn rejects_parent_path_escape() {
        let input = SERVER.replace("path = \".\"", "path = \"../outside\"");
        assert!(IndieBuildConfig::parse(&input).is_err());
    }

    #[test]
    fn rejects_env_secret_overlap() {
        let input = SERVER.replace("secret_env = []", "secret_env = [\"RUST_LOG\"]");
        assert!(IndieBuildConfig::parse(&input).is_err());
    }
}
