#![forbid(unsafe_code)]

use crate::{
    config::Config,
    error::CliError,
    indiebuild::{IndieBuildConfig, DEFAULT_CONFIG_PATH},
};

pub fn run(config: &Config) -> Result<(), CliError> {
    let contract = IndieBuildConfig::load(DEFAULT_CONFIG_PATH)?;
    if config.json {
        println!("{}", contract.normalized_json()?);
        return Ok(());
    }

    let target = contract.default();
    println!(
        "valid {}: role={:?} default_target={} profile={} platform={:?} path={}",
        DEFAULT_CONFIG_PATH,
        contract.repository_role,
        target.name,
        target.profile,
        target.platform,
        target.path
    );
    Ok(())
}
