#![forbid(unsafe_code)]

use crate::{config::Config, error::CliError, indiebuild::IndieBuildConfig, runtime};

pub fn run(config: &Config) -> Result<(), CliError> {
    let contract = IndieBuildConfig::load(&config.indiebuild_config)?;
    let machine = contract.normalized_json()?;
    let target = contract.default();
    let human = format!(
        "valid {}: role={:?} default_target={} profile={} platform={:?} path={}",
        config.indiebuild_config,
        contract.repository_role,
        target.name,
        target.profile,
        target.platform,
        target.path
    );
    runtime::emit(config.json, human, &machine)
}
