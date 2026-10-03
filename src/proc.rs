#![forbid(unsafe_code)]

//! Running the tools that own their own credentials.
//!
//! `gh` and `cloudflared` are invoked as subprocesses rather than reimplemented
//! against their REST APIs. Both already hold authenticated sessions, so
//! delegating means this CLI never reads, stores or passes a token, and never
//! needs a TLS stack.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::CliError;

/// Run a tool and capture its stdout, failing on a non-zero exit.
pub fn capture<S>(program: &str, args: &[S]) -> Result<String, CliError>
where
    S: AsRef<OsStr>,
{
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| CliError::Command(format!("cannot run {program}: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        return Err(CliError::Command(format!(
            "{program} failed ({}): {stderr}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a tool in the foreground, letting it own the terminal.
pub fn inherit<S>(program: &str, args: &[S], env: &[(String, String)]) -> Result<(), CliError>
where
    S: AsRef<OsStr>,
{
    let status = Command::new(program)
        .args(args)
        .envs(env.iter().map(|(key, value)| (key.clone(), value.clone())))
        .status()
        .map_err(|error| CliError::Command(format!("cannot run {program}: {error}")))?;
    if !status.success() {
        return Err(CliError::Command(format!("{program} exited with {status}")));
    }
    Ok(())
}

/// Start a long-running process in the background with its output in a log.
///
/// The child is reparented when this CLI exits; it is not a supervised daemon,
/// which is why the log path is reported rather than hidden.
pub fn background<S>(
    program: &str,
    args: &[S],
    env: &[(String, String)],
    log_path: &Path,
) -> Result<u32, CliError>
where
    S: AsRef<OsStr>,
{
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            CliError::Command(format!(
                "cannot create log directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let log = std::fs::File::create(log_path).map_err(|error| {
        CliError::Command(format!(
            "cannot create log file {}: {error}",
            log_path.display()
        ))
    })?;
    let errors = log
        .try_clone()
        .map_err(|error| CliError::Command(format!("cannot duplicate log handle: {error}")))?;
    let child = Command::new(program)
        .args(args)
        .envs(env.iter().map(|(key, value)| (key.clone(), value.clone())))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errors))
        .spawn()
        .map_err(|error| CliError::Command(format!("cannot start {program}: {error}")))?;
    Ok(child.id())
}

/// Whether a tool is on PATH, for telling the user what to install.
pub fn available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_returns_trimmed_stdout() {
        let output = capture("echo", &["hello"]).expect("echo runs");
        assert_eq!(output, "hello");
    }

    #[test]
    fn capture_surfaces_a_failing_exit_status() {
        let error = capture("sh", &["-c", "echo boom >&2; exit 3"]).expect_err("must fail");
        let message = error.to_string();
        assert!(message.contains("boom"), "{message}");
    }

    #[test]
    fn a_missing_tool_is_reported_not_silently_skipped() {
        assert!(!available("gha-indie-worker-tool-that-does-not-exist"));
        assert!(capture("gha-indie-worker-tool-that-does-not-exist", &["x"]).is_err());
    }
}
