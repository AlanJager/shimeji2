//! Local classification of Claude Code stdin and Codex `notify` argv.
//!
//! The line this crate sends is built only from the agent, the session id,
//! and the state. Prompt text, tool input, paths, and assistant text are
//! never copied out of the payload.

use serde_json::Value;
use shimeji_proto::{Agent, AgentState};

/// A payload reduced to what the socket is allowed to carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub agent: Agent,
    pub session: String,
    pub state: AgentState,
}

/// Classify one invocation.
///
/// Codex passes its JSON as an argument. When an argument is a JSON object
/// with a `type` field, that object wins and stdin is ignored. Otherwise the
/// Claude Code payload is read from `stdin`.
pub fn classify(stdin: &str, args: &[String]) -> Option<Classified> {
    if let Some(codex) = args.iter().rev().find_map(|arg| codex_arg(arg)) {
        return classify_codex(&codex);
    }
    let stdin = stdin.trim();
    if stdin.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_str(stdin).ok()?;
    classify_claude(&value)
}

fn codex_arg(arg: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(arg).ok()?;
    let obj = value.as_object()?;
    if obj.contains_key("type") {
        Some(value)
    } else {
        None
    }
}

/// Codex `notify` JSON. Only `agent-turn-complete` is a state today.
pub fn classify_codex(value: &Value) -> Option<Classified> {
    let obj = value.as_object()?;
    let kind = obj.get("type")?.as_str()?;
    let session = obj.get("thread-id").and_then(Value::as_str).unwrap_or("");
    let state = match kind {
        "agent-turn-complete" => AgentState::Done,
        _ => return None,
    };
    Some(Classified {
        agent: Agent::Codex,
        session: session.to_string(),
        state,
    })
}

/// Claude Code hook JSON from stdin.
pub fn classify_claude(value: &Value) -> Option<Classified> {
    let obj = value.as_object()?;
    let event = obj.get("hook_event_name")?.as_str()?;
    let session = obj.get("session_id").and_then(Value::as_str).unwrap_or("");
    let state = match event {
        "UserPromptSubmit" | "PreToolUse" | "SubagentStart" => AgentState::Working,
        "PermissionRequest" => AgentState::Waiting,
        "Notification" => notification_state(obj.get("notification_type").and_then(Value::as_str))?,
        "PostToolUse" => post_tool_state(obj)?,
        "PostToolUseFailure" | "StopFailure" | "PermissionDenied" => AgentState::Failed,
        "Stop" | "SubagentStop" => AgentState::Done,
        _ => return None,
    };
    Some(Classified {
        agent: Agent::ClaudeCode,
        session: session.to_string(),
        state,
    })
}

fn notification_state(kind: Option<&str>) -> Option<AgentState> {
    match kind {
        Some(
            "permission_prompt"
            | "idle_prompt"
            | "elicitation_dialog"
            | "elicitation_url_dialog"
            | "agent_needs_input",
        ) => Some(AgentState::Waiting),
        Some("agent_completed") => Some(AgentState::Done),
        _ => None,
    }
}

fn post_tool_state(obj: &serde_json::Map<String, Value>) -> Option<AgentState> {
    if obj.get("error").is_some_and(|value| !value.is_null()) {
        Some(AgentState::Failed)
    } else {
        Some(AgentState::Working)
    }
}
