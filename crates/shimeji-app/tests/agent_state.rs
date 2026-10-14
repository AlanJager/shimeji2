//! Shown state across sessions. Time is the caller's `now_ms`.

use shimeji_app::agent::{AgentTracker, DONE_FOR_MS, STALE_AFTER_MS, drives};
use shimeji_proto::{Agent, AgentState};

#[test]
fn waiting_outranks_working_and_done_lasts_three_seconds() {
    let mut tracker = AgentTracker::new();
    let t0 = 1_000_000_u64;
    tracker.observe(Agent::ClaudeCode, "a", AgentState::Working, t0);
    tracker.observe(Agent::ClaudeCode, "b", AgentState::Waiting, t0);
    assert_eq!(tracker.shown(t0), AgentState::Waiting);

    tracker.observe(Agent::ClaudeCode, "b", AgentState::Working, t0 + 1_000);
    assert_eq!(tracker.shown(t0 + 1_000), AgentState::Working);

    let done_at = t0 + 2_000;
    tracker.observe(Agent::ClaudeCode, "a", AgentState::Done, done_at);
    assert_eq!(tracker.shown(done_at), AgentState::Done);
    assert_eq!(tracker.shown(done_at + DONE_FOR_MS - 1), AgentState::Done);
    assert_eq!(tracker.shown(done_at + DONE_FOR_MS), AgentState::Working);
}

#[test]
fn a_silent_session_expires_to_idle() {
    let mut tracker = AgentTracker::new();
    tracker.observe(Agent::ClaudeCode, "only", AgentState::Working, 0);
    assert_eq!(tracker.shown(STALE_AFTER_MS), AgentState::Working);
    assert_eq!(tracker.shown(STALE_AFTER_MS + 1), AgentState::Idle);

    let mut tracker = AgentTracker::new();
    tracker.observe(Agent::ClaudeCode, "old", AgentState::Working, 0);
    tracker.observe(Agent::ClaudeCode, "live", AgentState::Waiting, 50_000);
    let at = 50_000 + STALE_AFTER_MS;
    assert_eq!(tracker.shown(at), AgentState::Waiting);
    assert_eq!(tracker.shown(at + 1), AgentState::Idle);
}

#[test]
fn coverage_matches_the_current_hook_surface() {
    assert!(drives(Agent::ClaudeCode, AgentState::Working));
    assert!(drives(Agent::ClaudeCode, AgentState::Waiting));
    assert!(drives(Agent::ClaudeCode, AgentState::Failed));
    assert!(drives(Agent::ClaudeCode, AgentState::Done));
    assert!(!drives(Agent::ClaudeCode, AgentState::Review));
    assert!(drives(Agent::Codex, AgentState::Done));
    assert!(!drives(Agent::Codex, AgentState::Waiting));
    assert!(!drives(Agent::Codex, AgentState::Working));
}
