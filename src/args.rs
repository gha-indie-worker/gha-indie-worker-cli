#![forbid(unsafe_code)]

use crate::error::CliError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Health,
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub command: Command,
    pub api_base: Option<String>,
    pub json: bool,
}

pub fn parse<I>(args: I) -> Result<Invocation, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut command = Command::Help;
    let mut api_base = None;
    let mut json = false;
    let mut items = args.into_iter().peekable();
    if let Some(first) = items.peek() {
        match first.as_str() {
            "health" => {
                command = Command::Health;
                items.next();
            }
            "status" => {
                command = Command::Status;
                items.next();
            }
            "-h" | "--help" | "help" => {
                command = Command::Help;
                items.next();
            }
            other if !other.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown command {other}")));
            }
            _ => {}
        }
    }
    for arg in items {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => command = Command::Help,
            flag if flag.starts_with("--api-base=") => {
                api_base = Some(flag.trim_start_matches("--api-base=").to_string());
            }
            other => return Err(CliError::Usage(format!("unknown flag {other}"))),
        }
    }
    Ok(Invocation {
        command,
        api_base,
        json,
    })
}

pub fn help_text() -> &'static str {
    "ghaiw — GHA Indie Worker CLI\n\nCommands:\n  health\n  status\n"
}
