#![forbid(unsafe_code)]

use std::io;
use std::sync::OnceLock;

use ores_clis_core::{
    CliPolicy, ColorRole, EmitDisposition, EnvironmentHints, LogLevel, ProtocolEmitter,
    RuntimePolicy, StreamRole, TerminalState, paint, top_level_io,
};

use crate::error::CliError;

static RUNTIME: OnceLock<RuntimePolicy> = OnceLock::new();

pub fn install(runtime: RuntimePolicy) {
    let _ = RUNTIME.set(runtime);
}

#[must_use]
pub fn current() -> RuntimePolicy {
    RUNTIME.get().copied().unwrap_or_else(|| {
        CliPolicy::default().resolve(TerminalState::detect(), EnvironmentHints::detect())
    })
}

pub fn emit(json: bool, human: impl std::fmt::Display, machine: &str) -> Result<(), CliError> {
    let runtime = current();
    let stdout = io::stdout();
    let mut emitter = ProtocolEmitter::new(stdout.lock(), StreamRole::Primary);
    let write = if json {
        emitter.emit_primary_machine_record(machine)
    } else {
        emitter.emit_primary_human_line(&paint(runtime.color_stdout(), ColorRole::Success, human))
    };
    match top_level_io(write).map_err(|error| CliError::Command(format!("output failed: {error}")))? {
        EmitDisposition::Written | EmitDisposition::ConsumerClosed => Ok(()),
    }
}

pub fn emit_error(message: impl std::fmt::Display) {
    let runtime = current();
    if !runtime.allows_log(LogLevel::Error) {
        return;
    }
    let stderr = io::stderr();
    let mut emitter = ProtocolEmitter::new(stderr.lock(), StreamRole::Diagnostics);
    let line = paint(runtime.color_stderr(), ColorRole::Error, message);
    let _ = top_level_io(emitter.emit_diagnostic_line(&line));
}
