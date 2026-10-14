//! Agent hook. Classifies a payload locally and sends one state line.
//!
//! The binary always exits 0 and writes nothing. A missing socket, a bad
//! directory, and a malformed payload are all silent.

mod classify;
mod send;

pub use classify::{Classified, classify, classify_claude, classify_codex};
pub use send::send_line;

use std::path::Path;
use std::time::Duration;

use shimeji_proto::{AgentEvent, SocketRole, encode_event, home_dir, prepare, valid_host};

/// How long `connect` may take before the hook gives up.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(100);

/// What one invocation did. `sent` is the line written to the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub sent: Option<String>,
}

/// Inputs for one hook invocation. `home` is the uid home, never `TMPDIR`.
pub struct RunInput<'a> {
    pub stdin: &'a str,
    pub args: &'a [String],
    pub bundle_id: Option<&'a str>,
    pub term_program: Option<&'a str>,
    pub home: &'a Path,
}

/// Classify, then send. Returns the line when the socket accepted it.
pub fn run(input: &RunInput<'_>) -> RunResult {
    let Some(classified) = classify(input.stdin, input.args) else {
        return RunResult { sent: None };
    };
    let event = AgentEvent {
        v: shimeji_proto::VERSION,
        agent: classified.agent,
        session: classified.session,
        state: classified.state,
        host: host_field(input.bundle_id, input.term_program),
    };
    let Ok(line) = encode_event(&event) else {
        return RunResult { sent: None };
    };
    let Ok(sock) = prepare(input.home, SocketRole::Client) else {
        return RunResult { sent: None };
    };
    if send_line(&sock, &line, CONNECT_TIMEOUT).is_err() {
        return RunResult { sent: None };
    }
    RunResult { sent: Some(line) }
}

/// Bundle id, then `TERM_PROGRAM`. A value that fails the pattern is skipped.
pub fn host_field(bundle_id: Option<&str>, term_program: Option<&str>) -> Option<String> {
    [bundle_id, term_program]
        .into_iter()
        .flatten()
        .find(|value| valid_host(value))
        .map(str::to_string)
}

/// The real hook: passwd home, process args, and the two host variables.
pub fn run_from_process(stdin: &str, args: &[String]) -> RunResult {
    let Ok(home) = home_dir() else {
        return RunResult { sent: None };
    };
    let bundle = std::env::var("__CFBundleIdentifier").ok();
    let term = std::env::var("TERM_PROGRAM").ok();
    run(&RunInput {
        stdin,
        args,
        bundle_id: bundle.as_deref(),
        term_program: term.as_deref(),
        home: &home,
    })
}
