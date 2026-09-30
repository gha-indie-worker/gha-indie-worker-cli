#![forbid(unsafe_code)]

pub mod health;
pub mod indiebuild_validate;
pub mod status;
pub mod tunnel;
pub mod verify;
pub mod webhook;
pub mod worker;

use crate::args::Command;
use crate::config::Config;
use crate::error::CliError;

pub fn dispatch(config: &Config, command: Command) -> Result<(), CliError> {
    match command {
        Command::Help => {
            print!("{}", crate::args::help_text());
            Ok(())
        }
        Command::Health => health::run(config),
        Command::Status => status::run(config),
        Command::IndieBuildValidate => indiebuild_validate::run(config),
        Command::WorkerUp => worker::up(config),
        Command::TunnelInit => tunnel::init(config),
        Command::TunnelUp => tunnel::up(config),
        Command::WebhookInstall => webhook::install(config),
        Command::Verify => verify::run(config),
    }
}
