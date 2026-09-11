#![forbid(unsafe_code)]

use crate::{config::Config, error::CliError, indiebuild::IndieBuildConfig};

pub fn run(config: &Config) -> Result<(), CliError> {
    let contract = IndieBuildConfig::load(&config.indiebuild_config)?;
    if config.json {
        println!("{}", contract.normalized_json()?);
        return Ok(());
    }

    let target = contract.default();
    println!(
        "valid {}: role={:?} default_target={} profile={} platform={:?} path={}",
        config.indiebuild_config,
        contract.repository_role,
        target.name,
        target.profile,
        target.platform,
        target.path
    );
    Ok(())
}
