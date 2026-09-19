#![forbid(unsafe_code)]

use std::path::Path;

use gha_indie_worker_cli::{args, commands, config, error::CliError, flags, runtime};
use ores_clis_core::{parse_shared_argv, EnvironmentHints, TerminalState};

fn main() {
    if let Err(err) = run() {
        runtime::emit_error(&err);
        std::process::exit(err.exit_code());
    }
}

fn run() -> Result<(), CliError> {
    let argv = std::env::args().collect::<Vec<_>>();
    let shared = parse_shared_argv(argv.iter().skip(1).cloned())
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let resolved = shared
        .policy
        .resolve(TerminalState::detect(), EnvironmentHints::detect());
    runtime::install(resolved);

    let legacy_output_explicit = std::env::var_os("GHA_INDIE_WORKER_JSON").is_some()
        || shared
            .passthrough
            .iter()
            .any(|argument| argument.starts_with("--json="));

    if shared
        .passthrough
        .iter()
        .any(|argument| matches!(argument.as_str(), "-h" | "--help" | "help"))
    {
        return runtime::emit_human(args::help_text());
    }

    // Read this before `shared.passthrough` is moved out below.
    let output_was_explicit = shared.output_was_explicit();
    let mut consumer_argv = Vec::with_capacity(shared.passthrough.len() + 1);
    consumer_argv.push(argv.first().cloned().unwrap_or_else(|| "giw".into()));
    consumer_argv.extend(shared.passthrough);
    let (command, env) = flags::apply_cli_flags_from(
        consumer_argv,
        std::env::vars().collect(),
        Path::new(".cli-flags.toml"),
    )?;
    let mut cfg = config::Config::from_env_map(&env)?;
    if output_was_explicit || !legacy_output_explicit {
        cfg.json = resolved.json();
    }
    commands::dispatch(&cfg, command)
}
