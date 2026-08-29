#![forbid(unsafe_code)]

use gha_indie_worker_cli::{args, commands, config, error::CliError, flags};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(err.exit_code());
    }
}

fn run() -> Result<(), CliError> {
    let argv = std::env::args().collect::<Vec<_>>();
    if argv
        .iter()
        .skip(1)
        .any(|argument| matches!(argument.as_str(), "-h" | "--help" | "help"))
    {
        print!("{}", args::help_text());
        return Ok(());
    }
    let (command, env) = flags::apply_cli_flags()?;
    let cfg = config::Config::from_env_map(&env)?;
    commands::dispatch(&cfg, command)
}
