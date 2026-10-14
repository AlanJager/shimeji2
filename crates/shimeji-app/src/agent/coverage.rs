//! States each agent can drive, for the Agents tab.
//!
//! Idle is the absence of a live session, not an event. Review is in the
//! priority order, and neither agent emits it with the current hook surface.

use shimeji_proto::{Agent, AgentState};

/// One row of the coverage table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCoverage {
    pub agent: Agent,
    pub states: &'static [AgentState],
}

/// Claude Code drives working, waiting, failed, and done.
/// Codex `notify` drives done.
pub fn coverage() -> &'static [AgentCoverage] {
    const CLAUDE: &[AgentState] = &[
        AgentState::Working,
        AgentState::Waiting,
        AgentState::Failed,
        AgentState::Done,
    ];
    const CODEX: &[AgentState] = &[AgentState::Done];
    &[
        AgentCoverage {
            agent: Agent::ClaudeCode,
            states: CLAUDE,
        },
        AgentCoverage {
            agent: Agent::Codex,
            states: CODEX,
        },
    ]
}

/// Whether `agent` can emit `state` with the current hook surface.
pub fn drives(agent: Agent, state: AgentState) -> bool {
    coverage()
        .iter()
        .find(|row| row.agent == agent)
        .is_some_and(|row| row.states.contains(&state))
}
