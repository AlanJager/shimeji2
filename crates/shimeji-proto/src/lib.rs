//! Versioned socket messages shared by the app, the hook, and forge.
//!
//! One connection carries one line, at most [`MAX_LINE_BYTES`]. Unknown
//! versions and unknown states are ignored. A session id is cleaned to at
//! most [`MAX_SESSION_CHARS`] printable characters. `host` is kept only when
//! it matches a bundle id or a short token.

mod socket_path;

pub use socket_path::{SocketError, SocketRole, home_dir, prepare, socket_path};

use serde::Serialize;
use serde_json::{Map, Value};

/// Current message version. Any other `v` is ignored.
pub const VERSION: u32 = 1;

/// One line, including the trailing newline the hook writes.
pub const MAX_LINE_BYTES: usize = 4096;

/// Session ids after cleaning. Longer input is truncated.
pub const MAX_SESSION_CHARS: usize = 128;

/// Install-request name, after the same cleaning as a session id.
pub const MAX_INSTALL_NAME_CHARS: usize = 128;

/// Install-request path. Longer paths are ignored.
pub const MAX_INSTALL_PATH_BYTES: usize = 1024;

/// Which coding agent produced an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Agent {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
}

/// Pet state. `Done` is the transient wave. The others are persistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Idle,
    Working,
    Review,
    Waiting,
    Failed,
    Done,
}

impl AgentState {
    /// `failed > waiting > review > working > idle`. `Done` is not ranked.
    pub fn rank(self) -> u8 {
        match self {
            Self::Failed => 5,
            Self::Waiting => 4,
            Self::Review => 3,
            Self::Working => 2,
            Self::Idle => 1,
            Self::Done => 0,
        }
    }
}

/// `{v, agent, session, state, host?}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub v: u32,
    pub agent: Agent,
    pub session: String,
    pub state: AgentState,
    pub host: Option<String>,
}

/// Forge install request (U16). The app confirms before it imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub v: u32,
    pub name: String,
    pub path: String,
}

/// A line the listener understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Event(AgentEvent),
    Install(InstallRequest),
}

/// Unknown versions and unknown states land here. They are not errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    Message(Message),
    Ignored,
}

/// The line is not a versioned message we can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Malformed,
    TooLong,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => write!(f, "malformed message"),
            Self::TooLong => write!(f, "message longer than {MAX_LINE_BYTES} bytes"),
        }
    }
}

/// Drop control characters, then keep at most [`MAX_SESSION_CHARS`].
pub fn clean_session(raw: &str) -> String {
    let mut out = String::new();
    let mut count = 0;
    for ch in raw.chars() {
        if !is_session_char(ch) {
            continue;
        }
        out.push(ch);
        count += 1;
        if count >= MAX_SESSION_CHARS {
            break;
        }
    }
    out
}

fn is_session_char(ch: char) -> bool {
    if ch.is_ascii() {
        (0x20..=0x7e).contains(&(ch as u8))
    } else {
        !ch.is_control()
    }
}

/// Bundle id (`com.apple.Terminal`) or a short token (`Apple_Terminal`, `iTerm.app`).
pub fn valid_host(value: &str) -> bool {
    valid_bundle_id(value) || valid_short_token(value)
}

fn valid_bundle_id(value: &str) -> bool {
    if value.len() > 255 || !value.contains('.') {
        return false;
    }
    let mut labels = 0;
    for label in value.split('.') {
        if !valid_bundle_label(label) {
            return false;
        }
        labels += 1;
    }
    labels >= 2
}

fn valid_bundle_label(label: &str) -> bool {
    let mut chars = label.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() => {}
        _ => return false,
    }
    label.len() <= 63 && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn valid_short_token(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 {
        return false;
    }
    let mut chars = value.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
}

/// Parse one socket line. A trailing newline is optional.
pub fn parse_line(line: &str) -> Result<ParseOutcome, ParseError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(ParseError::TooLong);
    }
    let line = line.trim_end_matches(['\r', '\n']);
    if line.is_empty() {
        return Err(ParseError::Malformed);
    }
    let value: Value = serde_json::from_str(line).map_err(|_| ParseError::Malformed)?;
    let obj = value.as_object().ok_or(ParseError::Malformed)?;
    let version = match obj.get("v") {
        Some(Value::Number(number)) => number.as_u64().ok_or(ParseError::Malformed)?,
        _ => return Err(ParseError::Malformed),
    };
    if version != u64::from(VERSION) {
        return Ok(ParseOutcome::Ignored);
    }
    if obj.get("type").and_then(Value::as_str) == Some("install") {
        return parse_install(obj);
    }
    parse_event(obj)
}

fn parse_event(obj: &Map<String, Value>) -> Result<ParseOutcome, ParseError> {
    let agent = match obj.get("agent").and_then(Value::as_str) {
        Some("claude-code") => Agent::ClaudeCode,
        Some("codex") => Agent::Codex,
        Some(_) => return Ok(ParseOutcome::Ignored),
        None => return Err(ParseError::Malformed),
    };
    let state = match obj.get("state").and_then(Value::as_str) {
        Some("idle") => AgentState::Idle,
        Some("working") => AgentState::Working,
        Some("review") => AgentState::Review,
        Some("waiting") => AgentState::Waiting,
        Some("failed") => AgentState::Failed,
        Some("done") => AgentState::Done,
        Some(_) => return Ok(ParseOutcome::Ignored),
        None => return Err(ParseError::Malformed),
    };
    let session = match obj.get("session") {
        Some(Value::String(text)) => clean_session(text),
        _ => return Err(ParseError::Malformed),
    };
    if session.is_empty() {
        return Ok(ParseOutcome::Ignored);
    }
    let host = match obj.get("host") {
        Some(Value::String(text)) if valid_host(text) => Some(text.clone()),
        _ => None,
    };
    Ok(ParseOutcome::Message(Message::Event(AgentEvent {
        v: VERSION,
        agent,
        session,
        state,
        host,
    })))
}

fn parse_install(obj: &Map<String, Value>) -> Result<ParseOutcome, ParseError> {
    let name = match obj.get("name") {
        Some(Value::String(text)) => clean_session(text),
        _ => return Err(ParseError::Malformed),
    };
    if name.is_empty() || name.chars().count() > MAX_INSTALL_NAME_CHARS {
        return Ok(ParseOutcome::Ignored);
    }
    let path = match obj.get("path") {
        Some(Value::String(text)) => text.clone(),
        _ => return Err(ParseError::Malformed),
    };
    if !valid_install_path(&path) {
        return Ok(ParseOutcome::Ignored);
    }
    Ok(ParseOutcome::Message(Message::Install(InstallRequest {
        v: VERSION,
        name,
        path,
    })))
}

fn valid_install_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_INSTALL_PATH_BYTES
        && !path.chars().any(|ch| ch.is_control() || ch == '\u{7f}')
}

/// One line, with a trailing newline. `host` is omitted when it does not match.
pub fn encode_event(event: &AgentEvent) -> Result<String, ParseError> {
    let session = clean_session(&event.session);
    if session.is_empty() {
        return Err(ParseError::Malformed);
    }
    let host = event.host.as_deref().filter(|value| valid_host(value));
    let wire = WireEvent {
        v: VERSION,
        agent: event.agent,
        session: &session,
        state: event.state,
        host,
    };
    finish_line(&wire)
}

/// One line, with a trailing newline.
pub fn encode_install(request: &InstallRequest) -> Result<String, ParseError> {
    let name = clean_session(&request.name);
    if name.is_empty() || !valid_install_path(&request.path) {
        return Err(ParseError::Malformed);
    }
    let wire = WireInstall {
        v: VERSION,
        r#type: "install",
        name: &name,
        path: &request.path,
    };
    finish_line(&wire)
}

fn finish_line<T: Serialize>(wire: &T) -> Result<String, ParseError> {
    let mut line = serde_json::to_string(wire).map_err(|_| ParseError::Malformed)?;
    if line.contains('\n') || line.len() + 1 > MAX_LINE_BYTES {
        return Err(ParseError::TooLong);
    }
    line.push('\n');
    Ok(line)
}

#[derive(Serialize)]
struct WireEvent<'a> {
    v: u32,
    agent: Agent,
    session: &'a str,
    state: AgentState,
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<&'a str>,
}

#[derive(Serialize)]
struct WireInstall<'a> {
    v: u32,
    r#type: &'a str,
    name: &'a str,
    path: &'a str,
}

/// Crate name. Dependents call this so the workspace link is real.
pub fn crate_name() -> &'static str {
    "shimeji-proto"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_version_and_state_are_ignored() {
        let version =
            parse_line(r#"{"v":2,"agent":"codex","session":"a","state":"done"}"#).unwrap();
        assert_eq!(version, ParseOutcome::Ignored);
        let state =
            parse_line(r#"{"v":1,"agent":"codex","session":"a","state":"dancing"}"#).unwrap();
        assert_eq!(state, ParseOutcome::Ignored);
        assert_eq!(parse_line("{").unwrap_err(), ParseError::Malformed);
    }

    #[test]
    fn session_control_characters_are_cleaned_and_truncated() {
        let raw = format!("ab\n\u{0001}cd{}", "x".repeat(200));
        let cleaned = clean_session(&raw);
        assert_eq!(cleaned.chars().count(), MAX_SESSION_CHARS);
        assert!(cleaned.starts_with("abcd"));
        assert!(!cleaned.contains('\n'));
        let line = format!(
            r#"{{"v":1,"agent":"claude-code","session":{},"state":"working"}}"#,
            serde_json::to_string(&raw).unwrap()
        );
        let ParseOutcome::Message(Message::Event(event)) = parse_line(&line).unwrap() else {
            panic!("expected an event");
        };
        assert_eq!(event.session, cleaned);
    }

    #[test]
    fn a_bad_host_is_dropped_and_a_bundle_id_is_kept() {
        let bad = parse_line(
            r#"{"v":1,"agent":"claude-code","session":"s","state":"working","host":"not a bundle"}"#,
        )
        .unwrap();
        let ParseOutcome::Message(Message::Event(event)) = bad else {
            panic!("expected an event");
        };
        assert_eq!(event.host, None);
        let good = parse_line(
            r#"{"v":1,"agent":"claude-code","session":"s","state":"working","host":"com.apple.Terminal"}"#,
        )
        .unwrap();
        let ParseOutcome::Message(Message::Event(event)) = good else {
            panic!("expected an event");
        };
        assert_eq!(event.host.as_deref(), Some("com.apple.Terminal"));
    }

    #[test]
    fn an_install_request_round_trips() {
        let request = InstallRequest {
            v: VERSION,
            name: "momo".to_string(),
            path: "/tmp/momo".to_string(),
        };
        let line = encode_install(&request).unwrap();
        assert!(line.ends_with('\n'));
        assert!(line.len() <= MAX_LINE_BYTES);
        let ParseOutcome::Message(Message::Install(parsed)) = parse_line(&line).unwrap() else {
            panic!("expected an install request");
        };
        assert_eq!(parsed, request);
    }

    #[test]
    fn a_line_over_4kb_is_an_error() {
        let line = format!("{}\n", "a".repeat(MAX_LINE_BYTES));
        assert_eq!(parse_line(&line).unwrap_err(), ParseError::TooLong);
    }
}
