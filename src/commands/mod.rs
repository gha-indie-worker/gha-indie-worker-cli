#![forbid(unsafe_code)]

pub mod health;
pub mod status;

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
    }
}

