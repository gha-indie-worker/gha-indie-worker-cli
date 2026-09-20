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

/// Long-running worker/tunnel children do not inherit the caller's whole shell.
///
/// These are runtime necessities, not credential channels. Explicit variables
/// supplied by the caller are added after this allowlist and therefore win.
const LAUNCH_AMBIENT_ALLOWLIST: [&str; 6] =
    ["PATH", "HOME", "TMPDIR", "TMP", "TEMP", "XDG_RUNTIME_DIR"];

fn isolated_launch_command(program: &str, env: &[(String, String)]) -> Command {
    let mut command = Command::new(program);
    command.env_clear();
    for key in LAUNCH_AMBIENT_ALLOWLIST {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.envs(
        env.iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    command
}

/// Run a tool and capture its stdout, failing on a non-zero exit.
///
/// Short-lived operator tools deliberately keep their ambient environment:
/// `gh` may use `GH_TOKEN` and both `gh` and `cloudflared` may use normal
/// per-user authentication/configuration. The stricter boundary below applies
/// to long-running worker/tunnel children.
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

/// Run a tool with `input` on its stdin and capture stdout.
///
/// This is how a secret reaches a subprocess: not argv, which any process on
/// the host can read, and not a file, which has a path, permissions and a
/// lifetime to get wrong.
pub fn capture_with_stdin<S>(program: &str, args: &[S], input: &[u8]) -> Result<String, CliError>
where
    S: AsRef<OsStr>,
{
    use std::io::Write;

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CliError::Command(format!("cannot run {program}: {error}")))?;
    // Dropping the handle closes the pipe, which is what lets the child see
    // end of input and finish.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input)
            .map_err(|error| CliError::Command(format!("cannot write to {program}: {error}")))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| CliError::Command(format!("cannot wait for {program}: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::Command(format!(
            "{program} failed ({}): {}",
            output.status,
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a long-lived tool in the foreground, letting it own the terminal.
///
/// The child starts from an empty environment, receives only a tiny runtime
/// allowlist, then receives the explicitly admitted service variables.
pub fn inherit<S>(program: &str, args: &[S], env: &[(String, String)]) -> Result<(), CliError>
where
    S: AsRef<OsStr>,
{
    let status = isolated_launch_command(program, env)
        .args(args)
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
/// which is why the log path is reported rather than hidden. Like `inherit`,
/// it does not receive unrelated credentials from the caller's shell.
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
    let child = isolated_launch_command(program, env)
        .args(args)
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
    use std::collections::BTreeSet;

    #[test]
    fn long_running_children_receive_only_allowlisted_or_explicit_environment() {
        let command = isolated_launch_command(
            "sh",
            &[
                ("EXPLICIT_SETTING".to_string(), "yes".to_string()),
                ("PATH".to_string(), "/explicit/path".to_string()),
            ],
        );
        let names: BTreeSet<String> = command
            .get_envs()
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect();

        assert!(names.contains("EXPLICIT_SETTING"));
        for name in &names {
            assert!(
                name == "EXPLICIT_SETTING"
                    || LAUNCH_AMBIENT_ALLOWLIST.contains(&name.as_str()),
                "unexpected inherited environment variable {name}"
            );
        }

        let path = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("PATH"))
            .and_then(|(_, value)| value)
            .map(|value| value.to_string_lossy().into_owned());
        assert_eq!(path.as_deref(), Some("/explicit/path"));
    }

    #[test]
    fn capture_returns_trimmed_stdout() {
        let output = capture("echo", &["hello"]).expect("echo runs");
        assert_eq!(output, "hello");
    }

    #[test]
    fn stdin_input_reaches_the_child_and_nothing_else_does() {
        let echoed =
            capture_with_stdin("cat", &[] as &[&str], b"secret-on-stdin").expect("cat runs");
        assert_eq!(echoed, "secret-on-stdin");
        // A failing child is still reported, and its message never includes
        // what was written to it.
        let error = capture_with_stdin(
            "sh",
            &["-c", "cat >/dev/null; echo nope >&2; exit 4"],
            b"secret-on-stdin",
        )
        .expect_err("must fail");
        assert!(error.to_string().contains("nope"));
        assert!(!error.to_string().contains("secret-on-stdin"));
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
