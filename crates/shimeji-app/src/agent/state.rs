//! Shown agent state across sessions.
//!
//! A session is stale when `now - timestamp > 120s`, and then it counts as
//! idle. The shown state is the highest priority among live sessions:
//! failed, waiting, review, working, idle. `done` is an overlay for 3s and
//! does not replace the session's underlying state.

use shimeji_proto::{Agent, AgentState};

/// A session with no newer event by this many milliseconds counts as idle.
pub const STALE_AFTER_MS: u64 = 120_000;

/// How long `done` stays on top of the underlying state.
pub const DONE_FOR_MS: u64 = 3_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    agent: Agent,
    id: String,
    state: AgentState,
    at_ms: u64,
}

/// Sessions seen so far, plus the transient done overlay.
#[derive(Debug, Clone, Default)]
pub struct AgentTracker {
    sessions: Vec<Session>,
    /// Exclusive end of the done overlay, in the same clock as `observe`.
    done_until_ms: Option<u64>,
}

impl AgentTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one event. `now_ms` is the caller's clock. `Done` starts the
    /// overlay and leaves the session's previous state in place.
    pub fn observe(&mut self, agent: Agent, session: &str, state: AgentState, now_ms: u64) {
        if state == AgentState::Done {
            self.done_until_ms = Some(now_ms.saturating_add(DONE_FOR_MS));
            self.upsert(agent, session, None, now_ms);
        } else {
            self.done_until_ms = None;
            self.upsert(agent, session, Some(state), now_ms);
        }
        self.sessions.retain(|item| {
            fresh(item.at_ms, now_ms) || (item.agent == agent && item.id == session)
        });
    }

    /// The state to show at `now_ms`.
    pub fn shown(&self, now_ms: u64) -> AgentState {
        if let Some(until) = self.done_until_ms {
            if now_ms < until {
                return AgentState::Done;
            }
        }
        let mut best = AgentState::Idle;
        for session in &self.sessions {
            if !fresh(session.at_ms, now_ms) {
                continue;
            }
            if session.state.rank() > best.rank() {
                best = session.state;
            }
        }
        best
    }

    fn upsert(&mut self, agent: Agent, session: &str, state: Option<AgentState>, now_ms: u64) {
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|item| item.agent == agent && item.id == session)
        {
            existing.at_ms = now_ms;
            if let Some(state) = state {
                existing.state = state;
            }
            return;
        }
        self.sessions.push(Session {
            agent,
            id: session.to_string(),
            state: state.unwrap_or(AgentState::Idle),
            at_ms: now_ms,
        });
    }
}

fn fresh(at_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(at_ms) <= STALE_AFTER_MS
}
