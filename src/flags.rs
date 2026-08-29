#![forbid(unsafe_code)]

use std::path::Path;

use crate::args::Command;
use crate::env_map::{merge_env, EnvMap};
use crate::error::CliError;
use flags2env::BundledFlags2Env;

pub fn parse_cli_flags(argv: &[String], config_path: &Path) -> Result<(Command, EnvMap), CliError> {
    let config_path = config_path
        .to_str()
        .ok_or_else(|| CliError::Config(".cli-flags.toml path is not valid UTF-8".into()))?;
    let parser = BundledFlags2Env::new();
    parser.audit_config(Some(config_path)).map_err(|error| {
        CliError::Config(format!("flags-2-env configuration audit failed: {error}"))
    })?;
    let parsed = parser
        .parse_structured(argv, Some(config_path))
        .map_err(|error| CliError::Config(format!("flags-2-env parse failed: {error}")))?;
    if !parsed.unknown_options.is_empty() {
        return Err(CliError::Usage(format!(
            "unknown command-line option(s): {}",
            parsed.unknown_options.join(", ")
        )));
    }
    if !parsed.errors.is_empty() {
        return Err(CliError::Usage(format!(
            "invalid command-line value(s): {}",
            parsed.errors.join("; ")
        )));
    }
    let command = match parsed.command.as_str() {
        "" | "help" => Command::Help,
        "health" => Command::Health,
        "status" => Command::Status,
        other => return Err(CliError::Usage(format!("unknown command {other}"))),
    };
    Ok((command, parsed.flags.into_iter().collect()))
}

pub fn apply_cli_flags() -> Result<(Command, EnvMap), CliError> {
    apply_cli_flags_from(
        std::env::args().collect(),
        std::env::vars().collect(),
        Path::new(".cli-flags.toml"),
    )
}

pub fn apply_cli_flags_from(
    argv: Vec<String>,
    initial: EnvMap,
    config_path: &Path,
) -> Result<(Command, EnvMap), CliError> {
    let (command, overrides) = parse_cli_flags(&argv, config_path)?;
    Ok((command, merge_env(initial, overrides)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env_map::value;

    fn config_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".cli-flags.toml")
    }

    #[test]
    fn health_command_merges_without_mutating_process_environment() {
        let before = std::env::var_os("ENV_MAP_PROBE");
        let (command, env) = apply_cli_flags_from(
            vec!["cli".into(), "health".into()],
            EnvMap::from([("ENV_MAP_PROBE".into(), "keep".into())]),
            &config_path(),
        )
        .expect("valid flags");
        assert_eq!(command, Command::Health);
        assert_eq!(value(&env, "ENV_MAP_PROBE"), Some("keep"));
        assert_eq!(std::env::var_os("ENV_MAP_PROBE"), before);
    }

    #[test]
    fn parse_failure_does_not_mutate_process_environment() {
        let before = std::env::var_os("ENV_MAP_PROBE");
        assert!(apply_cli_flags_from(
            vec![
                "cli".into(),
                "health".into(),
                "--this-flag-is-not-declared".into()
            ],
            EnvMap::from([("ENV_MAP_PROBE".into(), "keep".into())]),
            &config_path(),
        )
        .is_err());
        assert_eq!(std::env::var_os("ENV_MAP_PROBE"), before);
    }

    #[test]
    fn source_does_not_mutate_process_environment() {
        const SRC: &str = include_str!("flags.rs");
        let production = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(!production.contains("std::env::set_var"));
        assert!(!production.contains("env::set_var"));
    }
}
