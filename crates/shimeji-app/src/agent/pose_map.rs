//! Resolve an agent state to a behavior or action name from `pose_map.toml`.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use shimeji_core::model::Schema;
use shimeji_proto::AgentState;
use toml_edit::DocumentMut;

/// A name the pack actually has, or badge-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoseChoice {
    /// Behavior or action name. The first candidate present in the pack.
    Named(String),
    BadgeOnly,
}

/// First candidate for `schema` and `state` that appears in `names`.
pub fn resolve<'a, I>(schema: Schema, state: AgentState, names: I) -> PoseChoice
where
    I: IntoIterator<Item = &'a str>,
{
    let present: HashSet<&str> = names.into_iter().collect();
    for candidate in candidates(schema, state) {
        if present.contains(candidate.as_str()) {
            return PoseChoice::Named(candidate.clone());
        }
    }
    PoseChoice::BadgeOnly
}

fn candidates(schema: Schema, state: AgentState) -> &'static [String] {
    let table = table();
    table
        .get(&(schema_key(schema), state_key(state)))
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn schema_key(schema: Schema) -> &'static str {
    match schema {
        Schema::Japanese => "japanese",
        Schema::English => "english",
    }
}

fn state_key(state: AgentState) -> &'static str {
    match state {
        AgentState::Idle => "idle",
        AgentState::Working => "working",
        AgentState::Review => "review",
        AgentState::Waiting => "waiting",
        AgentState::Failed => "failed",
        AgentState::Done => "done",
    }
}

fn table() -> &'static HashMap<(&'static str, &'static str), Vec<String>> {
    static TABLE: OnceLock<HashMap<(&'static str, &'static str), Vec<String>>> = OnceLock::new();
    TABLE.get_or_init(parse_table)
}

fn parse_table() -> HashMap<(&'static str, &'static str), Vec<String>> {
    let doc: DocumentMut = include_str!("pose_map.toml")
        .parse()
        .expect("pose_map.toml");
    let mut out = HashMap::new();
    for (schema, item) in doc.iter() {
        let Some(table) = item.as_table() else {
            continue;
        };
        let schema: &'static str = match schema {
            "japanese" => "japanese",
            "english" => "english",
            _ => continue,
        };
        for (state, value) in table.iter() {
            let state: &'static str = match state {
                "idle" => "idle",
                "working" => "working",
                "review" => "review",
                "waiting" => "waiting",
                "failed" => "failed",
                "done" => "done",
                _ => continue,
            };
            let Some(array) = value.as_array() else {
                continue;
            };
            let names = array
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect();
            out.insert((schema, state), names);
        }
    }
    out
}
