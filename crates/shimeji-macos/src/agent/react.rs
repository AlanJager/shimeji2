//! Deliver one socket event on the main thread: badge, then a pose at the
//! next tick. The badge moves even while the clock is stopped.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::DefinedClass;
use objc2::MainThreadMarker;
use shimeji_app::Command;
use shimeji_app::agent::{AgentTracker, ForgeInstall, Freshness, PoseChoice, freshness, resolve};
use shimeji_app::import::Outcome;
use shimeji_core::MascotId;
use shimeji_proto::{AgentEvent, AgentState, InstallRequest, home_dir};

use crate::bridge::{AppState, ShellClock};
use crate::platform::coords::Rect;
use crate::ui::actions;

use super::badge::{self, Badge};
use super::listener::{self, Listener};

/// Listener, tracker, and the pose waiting for a mascot id.
pub struct Session {
    pub listener: Option<Listener>,
    pub status: String,
    pub home: Option<PathBuf>,
    pub tracker: AgentTracker,
    pub shown: Option<AgentState>,
    pub host: Option<String>,
    pub seen: bool,
    pub posed_for: Option<AgentState>,
    pub pending_pose: Option<String>,
    pub asked: Option<String>,
    pub pose_announced: bool,
    pub last_session: String,
    pub mascot: Option<MascotId>,
    pub alive: Arc<AtomicBool>,
    pub forge: ForgeInstall,
}

impl Session {
    pub fn new(home: Option<PathBuf>) -> Self {
        Self {
            listener: None,
            status: "off".to_string(),
            home,
            tracker: AgentTracker::new(),
            shown: None,
            host: None,
            seen: false,
            posed_for: None,
            pending_pose: None,
            asked: None,
            pose_announced: false,
            last_session: String::new(),
            mascot: None,
            alive: Arc::new(AtomicBool::new(true)),
            forge: ForgeInstall::new(),
        }
    }
}

pub(crate) fn hooks_word() -> &'static str {
    let Some((claude, codex, hook)) = hook_paths() else {
        return "unavailable";
    };
    match freshness(&claude, &codex, &hook) {
        Freshness::NotInstalled => "not_installed",
        Freshness::Current => "current",
        Freshness::OutOfDate => "out_of_date",
    }
}

pub(crate) fn hook_paths() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let home = home_dir().ok()?;
    let hook = std::env::current_exe().ok()?;
    Some((
        home.join(".claude/settings.json"),
        home.join(".codex/config.toml"),
        hook,
    ))
}

pub(crate) fn config_paths() -> Option<(PathBuf, PathBuf)> {
    hook_paths().map(|(claude, codex, _)| (claude, codex))
}

pub(crate) fn hook_bin() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("shimeji-hook"))
}

/// Bind the listener. A failed `prepare` leaves the status as the error text
/// and starts no thread.
pub(crate) fn start(clock: &ShellClock) {
    let home = {
        let state = clock.ivars().state.borrow();
        if state.agent.listener.is_some() {
            return;
        }
        state.agent.home.clone()
    };
    let Some(home) = home else {
        let message = "could not read the home directory".to_string();
        emit(&format!("agent_listener={message}"));
        clock.ivars().state.borrow_mut().agent.status = message;
        return;
    };
    let ptr = std::ptr::from_ref(clock) as usize;
    let alive = Arc::clone(&clock.ivars().state.borrow().agent.alive);
    alive.store(true, Ordering::Release);
    match listener::start(&home, true, move |inbound| {
        // `alive` is cleared before the clock is dropped. A callback that
        // loses that race returns without touching the pointer.
        if !alive.load(Ordering::Acquire) {
            return;
        }
        let clock = unsafe { &*(ptr as *const ShellClock) };
        match inbound {
            listener::Inbound::Event(event) => on_event(clock, event),
            listener::Inbound::Install(request) => on_install(clock, request),
        }
    }) {
        Ok(listener) => {
            emit("agent_listener=listening");
            emit(&format!("agent_home={}", home.display()));
            let mut state = clock.ivars().state.borrow_mut();
            state.agent.status = "listening".to_string();
            state.agent.listener = Some(listener);
        }
        Err(error) => {
            emit(&format!("agent_listener={error}"));
            clock.ivars().state.borrow_mut().agent.status = error;
        }
    }
}

pub(crate) fn stop(clock: &ShellClock) {
    let listener = {
        let mut state = clock.ivars().state.borrow_mut();
        state.agent.alive.store(false, Ordering::Release);
        state.agent.status = "off".to_string();
        state.agent.seen = false;
        state.agent.shown = None;
        state.agent.pending_pose = None;
        state.agent.listener.take()
    };
    let was = listener.is_some();
    drop(listener);
    if was {
        emit("agent_listener=off");
    }
    place_badge(clock);
}

fn on_install(clock: &ShellClock, request: InstallRequest) {
    let prompt = ForgeInstall::prompt(&request);
    {
        let mut state = clock.ivars().state.borrow_mut();
        if state.agent.forge.offer(request).is_err() {
            emit("forge_install=busy");
            return;
        }
    }
    let accepted = match MainThreadMarker::new() {
        Some(mtm) => crate::ui::import_flow::ask_forge_install(mtm, &prompt),
        None => false,
    };
    if !accepted {
        clock.ivars().state.borrow_mut().agent.forge.ignore();
        emit("forge_install=ignored");
        return;
    }
    let packs_root = clock.ivars().state.borrow().support_dir.clone();
    let outcome = {
        let mut state = clock.ivars().state.borrow_mut();
        state.agent.forge.confirm(&packs_root)
    };
    let Some(outcome) = outcome else {
        return;
    };
    let installed = matches!(outcome, Outcome::Success(_) | Outcome::Degraded(_));
    if installed {
        let text = crate::ui::import_flow::banner(&outcome);
        crate::ui::actions::set_import_banner(clock, &text);
        println!("import_banner={text}");
        emit("forge_install=installed");
        crate::ui::actions::note_import(clock, outcome);
        return;
    }
    if let Outcome::Error { message } = &outcome {
        crate::ui::actions::set_import_banner(clock, message);
        emit("forge_install=rejected");
        return;
    }
    if let Outcome::Duplicate { id } = &outcome {
        let text = format!("{id} is already installed.");
        crate::ui::actions::set_import_banner(clock, &text);
        emit("forge_install=rejected");
    }
}

pub(crate) fn on_event(clock: &ShellClock, event: AgentEvent) {
    let now_ms = {
        let state = clock.ivars().state.borrow();
        u64::try_from(state.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    };
    let spawn = {
        let mut state = clock.ivars().state.borrow_mut();
        if event.host.is_some() {
            state.agent.host = event.host.clone();
        }
        state
            .agent
            .tracker
            .observe(event.agent, &event.session, event.state, now_ms);
        let shown = state.agent.tracker.shown(now_ms);
        let changed = state.agent.shown != Some(shown);
        state.agent.shown = Some(shown);
        state.agent.seen = true;
        state.agent.last_session = event.session.clone();
        let (pack, character) = chosen(&state);
        let id = first_id(&state, &pack, &character);
        state.agent.mascot = id;
        let pose = if changed {
            pose_name(&state, &pack, &character, shown)
        } else {
            None
        };
        let mut spawn = None;
        if changed && state.agent.posed_for != Some(shown) {
            state.agent.posed_for = Some(shown);
            if let Some(name) = pose.clone() {
                state.agent.asked = Some(name.clone());
                state.agent.pose_announced = false;
                if let Some(id) = id {
                    push_interject(&mut state, id, &name);
                } else {
                    state.agent.pending_pose = Some(name);
                    spawn = Some((pack, character));
                }
            }
        }
        let pose_text = pose.unwrap_or_else(|| "-".to_string());
        let mascot_text = id
            .map(|id| id.0.to_string())
            .unwrap_or_else(|| "-".to_string());
        let applied = if changed { 1 } else { 0 };
        emit(&format!(
            "agent_event session={} state={} pose={} mascot={} applied={applied}",
            event.session,
            badge::state_word(shown),
            pose_text,
            mascot_text,
        ));
        spawn
    };
    if let Some((pack, character)) = spawn {
        actions::spawn_character(clock, &pack, &character);
    }
    place_badge(clock);
}

/// Queue a pose that was waiting for the mascot id `Spawn` reports.
pub(crate) fn flush_pending(state: &mut AppState) {
    let Some(name) = state.agent.pending_pose.clone() else {
        return;
    };
    let (pack, character) = chosen(state);
    let Some(id) = first_id(state, &pack, &character) else {
        return;
    };
    state.agent.pending_pose = None;
    state.agent.mascot = Some(id);
    state.agent.asked = Some(name.clone());
    state.agent.pose_announced = false;
    push_interject(state, id, &name);
}

pub(crate) fn note_pose(state: &mut AppState) {
    let Some(wanted) = state.agent.asked.clone() else {
        return;
    };
    if state.agent.pose_announced {
        return;
    }
    let (pack, character) = chosen(state);
    let Some(id) = state
        .agent
        .mascot
        .or_else(|| first_id(state, &pack, &character))
    else {
        return;
    };
    let behavior = behavior_of(state, &pack, &character, id);
    if behavior.as_deref() == Some(wanted.as_str()) {
        emit(&format!(
            "agent_pose session={} behavior={wanted}",
            state.agent.last_session
        ));
        state.agent.pose_announced = true;
    }
}

pub(crate) fn place_badge(clock: &ShellClock) {
    let placed = {
        let state = clock.ivars().state.borrow();
        if !state.settings.agents || !state.agent.seen {
            None
        } else {
            let shown = state.agent.shown;
            let (pack, character) = chosen(&state);
            let id = state
                .agent
                .mascot
                .or_else(|| first_id(&state, &pack, &character));
            let sprite = id.and_then(|id| sprite_of(&state, &pack, &character, id));
            Some((shown, sprite))
        }
    };
    let Some((Some(shown), Some(sprite))) = placed else {
        hide_badge(clock);
        return;
    };
    show_badge(clock, sprite, shown);
}

fn hide_badge(clock: &ShellClock) {
    if let Some(badge) = clock.ivars().badge.borrow_mut().as_mut() {
        badge.hide();
    }
}

fn show_badge(clock: &ShellClock, sprite: Rect, state: AgentState) {
    let mtm = objc2::MainThreadMarker::new();
    let Some(mtm) = mtm else {
        return;
    };
    let mut slot = clock.ivars().badge.borrow_mut();
    if slot.is_none() {
        let target = std::ptr::from_ref(clock).cast::<objc2::runtime::AnyObject>();
        *slot = Some(Badge::new(mtm, unsafe { &*target }));
    }
    if let Some(badge) = slot.as_mut() {
        badge.show(sprite, state);
    }
}

fn chosen(state: &AppState) -> (String, String) {
    let spec = state.settings.agent_character.as_str();
    if let Some((pack, name)) = spec.split_once('/') {
        if !pack.is_empty() && !name.is_empty() {
            return (pack.to_string(), name.to_string());
        }
    }
    (state.pack_id.clone(), state.character_name.clone())
}

fn first_id(state: &AppState, pack: &str, name: &str) -> Option<MascotId> {
    frames(state, pack, name)
        .into_iter()
        .map(|frame| frame.id)
        .min_by_key(|id| id.0)
}

fn behavior_of(state: &AppState, pack: &str, name: &str, id: MascotId) -> Option<String> {
    frames(state, pack, name)
        .into_iter()
        .find(|frame| frame.id == id)
        .map(|frame| frame.behavior)
}

fn frames(state: &AppState, pack: &str, name: &str) -> Vec<shimeji_core::engine::MascotFrame> {
    if state.pack_id == pack && state.character_name == name {
        state.runtime.frames()
    } else {
        state
            .extras
            .iter()
            .find(|extra| extra.pack_id == pack && extra.character == name)
            .map(|extra| extra.runtime.frames())
            .unwrap_or_default()
    }
}

fn sprite_of(state: &AppState, pack: &str, name: &str, id: MascotId) -> Option<Rect> {
    if state.pack_id == pack && state.character_name == name {
        state.renderer.sprite_cocoa(id)
    } else {
        state
            .extras
            .iter()
            .find(|extra| extra.pack_id == pack && extra.character == name)
            .and_then(|extra| extra.renderer.sprite_cocoa(id))
    }
}

fn pose_name(state: &AppState, pack: &str, name: &str, shown: AgentState) -> Option<String> {
    let cache = state.caches.iter().find(|item| item.id == pack)?;
    let character = cache
        .pack
        .characters
        .iter()
        .find(|item| item.name == name)?;
    let names = character
        .behaviors
        .iter()
        .map(|(key, _)| key)
        .chain(character.actions.iter().map(|(key, _)| key));
    match resolve(character.behavior_schema, shown, names) {
        PoseChoice::Named(name) => Some(name),
        PoseChoice::BadgeOnly => None,
    }
}

fn push_interject(state: &mut AppState, id: MascotId, name: &str) {
    let (pack, character) = chosen(state);
    let command = Command::InterjectBehavior {
        mascot: id,
        behavior: name.to_string(),
    };
    if state.pack_id == pack && state.character_name == character {
        state.runtime.push(command);
    } else if let Some(extra) = state
        .extras
        .iter_mut()
        .find(|extra| extra.pack_id == pack && extra.character == character)
    {
        extra.runtime.push(command);
    }
}

fn emit(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

pub(crate) fn listener_sentence(status: &str) -> String {
    match status {
        "off" => "Listener is off".to_string(),
        "listening" => "Listener is running".to_string(),
        other => other.to_string(),
    }
}

pub(crate) fn hooks_sentence(word: &str) -> String {
    match word {
        "out_of_date" => "Hooks out of date — Repair".to_string(),
        "current" => "Hooks are current.".to_string(),
        "not_installed" => "Hooks are not installed.".to_string(),
        _ => "Hook status is unavailable.".to_string(),
    }
}

pub(crate) fn coverage_text() -> String {
    use shimeji_app::agent::coverage;
    use shimeji_proto::Agent;
    let mut lines = Vec::new();
    for row in coverage() {
        let agent = match row.agent {
            Agent::ClaudeCode => "Claude Code",
            Agent::Codex => "Codex",
        };
        let states = row
            .states
            .iter()
            .map(|state| badge::state_word(*state))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("{agent}: {states}"));
    }
    lines.push("Review is ranked. Neither agent emits it.".to_string());
    lines.join("\n")
}

/// The review pane shows the hooks diff and the notify diff, and nothing else.
pub(crate) fn review_text(claude_diff: &str, codex_diff: &str) -> String {
    match (claude_diff.is_empty(), codex_diff.is_empty()) {
        (true, true) => "No changes.".to_string(),
        (false, true) => claude_diff.to_string(),
        (true, false) => codex_diff.to_string(),
        (false, false) => format!("{claude_diff}\n{codex_diff}"),
    }
}
