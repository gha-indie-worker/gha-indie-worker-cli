#![forbid(unsafe_code)]

use gha_indie_worker_cli::{args, commands, config, error::CliError};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(err.exit_code());
    }
}

fn run() -> Result<(), CliError> {
    let invocation = args::parse(std::env::args().skip(1))?;
    let cfg = config::Config::load(&invocation)?;
    commands::dispatch(&cfg, invocation.command)
}

