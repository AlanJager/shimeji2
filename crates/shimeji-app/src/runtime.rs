//! Fixed-step clock the macOS shell drives.
//!
//! Timestamps are milliseconds. The step is 40 ms. One call runs at most two
//! ticks, then drops the leftover so a stall does not replay a backlog.
//! [`Command::MoveFrozen`], [`Command::Pause`], [`Command::SpawnAt`], and
//! [`Command::SetMascotCap`] apply as they arrive. Every other command waits
//! for a tick boundary and runs once, before the burst.

use shimeji_core::diagnostics::Diagnostic;
use shimeji_core::env::{Point, Snapshot, WorkRect};
use shimeji_core::model::{ActionKind, Character, Pose};
use shimeji_core::window_control::{SoundEvent, WindowControl, WindowEvent, WindowMove};
use shimeji_core::{
    Engine, EngineSettings, MascotDelta, MascotFrame, MascotId, MouseButton, NopWindow, TickDelta,
};

/// One engine step, in milliseconds.
pub const TICK_MS: f64 = 40.0;

/// Ticks one `on_frame` will run before it drops the rest of the gap.
pub const MAX_TICKS_PER_FRAME: u32 = 2;

/// What the shell asks the runtime to do.
#[derive(Debug, Clone)]
pub enum Command {
    Spawn,
    Dismiss(MascotId),
    /// `true` freezes the clock. `false` resumes it with an empty accumulator.
    Pause(bool),
    Press {
        mascot: Option<MascotId>,
        button: MouseButton,
    },
    Release {
        mascot: Option<MascotId>,
        button: MouseButton,
    },
    /// Anchor write outside the tick. Applied even while paused.
    MoveFrozen {
        id: MascotId,
        anchor: Point,
    },
    /// `setBehavior` on one live mascot, or on every live mascot when `id` is
    /// `None` (the reference's Follow Cursor). Runs at the next tick boundary.
    SetBehavior {
        id: Option<MascotId>,
        name: String,
    },
    /// Enable or disable a toggleable behavior for later selection.
    SetBehaviorEnabled {
        name: String,
        enabled: bool,
    },
    /// Place a mascot at `anchor` before the next tick. Applied even while
    /// paused, so Reduce Motion can stand a pet on the floor without a fall.
    SpawnAt(Point),
    /// Live-mascot cap. Applied immediately, including while the clock is stopped.
    SetMascotCap(usize),
    Settings(EngineSettings),
    /// Result of a window move requested on an earlier frame.
    Window(WindowEvent),
    /// Enter `behavior` on one mascot at the next tick, before the environment
    /// update. A behavior name is entered whole. An action name runs once.
    InterjectBehavior {
        mascot: MascotId,
        behavior: String,
    },
}

/// What one `on_frame` produced. Borrows the runtime's buffers.
#[derive(Debug)]
pub struct Frame<'a> {
    pub ticks: u32,
    pub resynced: bool,
    pub deltas: &'a [TickDelta],
    pub window_moves: &'a [WindowMove],
    pub sounds: &'a [SoundEvent],
    pub spawned: &'a [MascotId],
    pub diagnostics: &'a [Diagnostic],
    pub frozen: &'a [MascotDelta],
}

/// Platform-free facade. `W` is the non-blocking window control.
pub struct Runtime<W: WindowControl = NopWindow> {
    engine: Engine,
    windows: W,
    paused: bool,
    started: bool,
    last_ms: f64,
    accum: f64,
    commands: Vec<Command>,
    diag_cursor: usize,
    deltas: Vec<TickDelta>,
    window_moves: Vec<WindowMove>,
    sounds: Vec<SoundEvent>,
    spawned: Vec<MascotId>,
    diagnostics: Vec<Diagnostic>,
    frozen: Vec<MascotDelta>,
    companion: Option<Vec<MascotFrame>>,
    companion_pose: Option<Pose>,
    companion_fallback_pose: Option<Pose>,
}

impl Runtime<NopWindow> {
    pub fn new(character: &Character, seed: i64) -> Self {
        Self::with_windows(character, seed, NopWindow)
    }
}

impl<W: WindowControl> Runtime<W> {
    pub fn with_windows(character: &Character, seed: i64, windows: W) -> Self {
        let engine = Engine::new(character, seed);
        let diag_cursor = engine.diagnostics().len();
        Self {
            engine,
            windows,
            paused: false,
            started: false,
            last_ms: 0.0,
            accum: 0.0,
            commands: Vec::new(),
            diag_cursor,
            deltas: Vec::new(),
            window_moves: Vec::new(),
            sounds: Vec::new(),
            spawned: Vec::new(),
            diagnostics: Vec::new(),
            frozen: Vec::new(),
            companion: None,
            companion_pose: quiet_pose(character),
            companion_fallback_pose: fallback_pose(character),
        }
    }

    pub fn push(&mut self, command: Command) {
        self.commands.push(command);
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Freeze or resume without waiting for a tick. The power controller uses
    /// this when it is about to stop the clock, so a later resume cannot be
    /// undone by a `Pause(true)` that never got a frame to run in.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.accum = 0.0;
    }

    pub fn population(&self) -> usize {
        self.engine.population()
    }

    pub fn frames(&self) -> Vec<MascotFrame> {
        self.companion
            .clone()
            .unwrap_or_else(|| self.engine.frames())
    }

    /// A static companion presentation, independent of the user's pause flag.
    /// No engine ticks, sounds, window moves, or automatic breeding run while
    /// enabled. Direct lifecycle/settings commands and frozen dragging remain
    /// usable. Shells must redraw every frame on a mode transition.
    pub fn set_companion_mode(&mut self, enabled: bool, snapshot: &Snapshot) {
        if enabled == self.companion_mode() {
            return;
        }
        self.accum = 0.0;
        // Rebase the animation clock on the next callback, so leaving quiet
        // mode cannot simulate the elapsed focus session as animation ticks.
        self.started = false;
        // Queued input and transient poses never cross a focus boundary.
        // Lifecycle commands stay queued for on_frame, preserving its spawned
        // and frozen output rather than consuming callbacks in this setter.
        self.commands.retain(|command| {
            !matches!(
                command,
                Command::Press { .. }
                    | Command::Release { .. }
                    | Command::SetBehavior { .. }
                    | Command::InterjectBehavior { .. }
                    | Command::Window(_)
            )
        });
        if enabled {
            self.engine.apply(snapshot);
            self.engine.flush_pending();
            self.companion = Some(Vec::new());
            self.refresh_companion(snapshot);
            self.engine.cancel_interactions();
        } else {
            self.companion = None;
        }
    }

    pub fn companion_mode(&self) -> bool {
        self.companion.is_some()
    }

    fn apply_companion_commands(&mut self, snapshot: &Snapshot) {
        self.engine.apply(snapshot);
        for command in std::mem::take(&mut self.commands) {
            match command {
                Command::Spawn => {
                    self.engine.spawn_at(park_anchor(snapshot.cursor, snapshot));
                }
                Command::SpawnAt(anchor) => {
                    self.engine.spawn_at(park_anchor(anchor, snapshot));
                }
                Command::Dismiss(id) => {
                    self.engine.dismiss(id);
                    // Frozen companions have no later engine tick to release
                    // capacity before the next queued spawn is checked.
                    self.engine.flush_pending();
                }
                Command::Pause(paused) => self.set_paused(paused),
                Command::MoveFrozen { id, anchor } => {
                    self.engine.move_frozen(id, anchor);
                    if let Some(frame) = self
                        .companion
                        .as_mut()
                        .and_then(|frames| frames.iter_mut().find(|frame| frame.id == id))
                    {
                        frame.anchor = anchor;
                    }
                }
                Command::Settings(settings) => self.engine.set_settings(settings),
                Command::SetMascotCap(cap) => self.engine.set_mascot_cap(cap),
                Command::SetBehaviorEnabled { name, enabled } => {
                    self.engine.set_behavior_enabled(&name, enabled)
                }
                // A dedicated static presentation owns the pose; user input
                // moves it through MoveFrozen. Never queue delayed throws or
                // stale Agent reactions for the end of the focus session.
                Command::Press { .. }
                | Command::Release { .. }
                | Command::SetBehavior { .. }
                | Command::InterjectBehavior { .. }
                | Command::Window(_) => {}
            }
        }
        self.engine.flush_pending();
        self.engine
            .drain_effects(&mut self.window_moves, &mut self.sounds, &mut self.spawned);
        self.window_moves.clear();
        self.sounds.clear();
        self.refresh_companion(snapshot);
    }

    fn refresh_companion(&mut self, snapshot: &Snapshot) {
        let Some(previous) = self.companion.take() else {
            return;
        };
        let mut frames = self.engine.frames();
        for frame in &mut frames {
            if let Some(old) = previous.iter().find(|old| old.id == frame.id) {
                *frame = old.clone();
                if snapshot
                    .screens
                    .iter()
                    .any(|screen| screen.width > 0 && screen.height > 0)
                    && !snapshot.screens.iter().any(|screen| {
                        let rect = screen_rect(screen);
                        frame.anchor.x >= rect.x
                            && frame.anchor.x <= rect.x.saturating_add(rect.width)
                            && frame.anchor.y >= rect.y
                            && frame.anchor.y <= rect.y.saturating_add(rect.height)
                    })
                {
                    frame.anchor = park_anchor(frame.anchor, snapshot);
                    self.engine.move_frozen(frame.id, frame.anchor);
                }
            } else {
                frame.anchor = park_anchor(frame.anchor, snapshot);
                self.engine.move_frozen(frame.id, frame.anchor);
                let pose = self.companion_pose.as_ref().or_else(|| {
                    if frame.image.is_none() {
                        self.companion_fallback_pose.as_ref()
                    } else {
                        None
                    }
                });
                if let Some(pose) = pose {
                    frame.image = pose
                        .image_source
                        .clone()
                        .or_else(|| pose.image.as_ref().map(|image| image.source.clone()));
                    let width = if frame.look_right {
                        pose.image_right
                            .as_ref()
                            .or(pose.right_sibling.as_ref())
                            .or(pose.image.as_ref())
                    } else {
                        pose.image.as_ref()
                    }
                    .map_or(0, |image| image.width);
                    frame.image_anchor = Some(Point::new(
                        if frame.look_right && width > 0 {
                            width.saturating_sub(pose.anchor_x)
                        } else {
                            pose.anchor_x
                        },
                        pose.anchor_y,
                    ));
                }
            }
            self.frozen.push(MascotDelta {
                id: frame.id,
                anchor_changed: true,
                image_changed: true,
            });
        }
        self.companion = Some(frames);
    }

    pub fn settings(&self) -> EngineSettings {
        self.engine.settings()
    }

    /// Advance the clock to `now_ms`.
    ///
    /// The first call only records the origin. Later calls add the gap to the
    /// accumulator. Each full 40 ms is one tick, up to two. A leftover of
    /// another full step is dropped (`resynced`).
    ///
    /// `SpawnAt` builds the first behavior immediately. When one is queued,
    /// the snapshot is published first, so that selection sees the screens
    /// and the floor. A frame with no `SpawnAt` leaves the engine untouched
    /// until a tick.
    pub fn on_frame<'a>(&'a mut self, now_ms: f64, snapshot: &Snapshot) -> Frame<'a> {
        self.deltas.clear();
        self.window_moves.clear();
        self.sounds.clear();
        self.spawned.clear();
        self.diagnostics.clear();
        self.frozen.clear();

        if self.companion_mode() {
            self.apply_companion_commands(snapshot);
            self.started = true;
            self.last_ms = now_ms;
            self.accum = 0.0;
            self.take_diagnostics();
            return self.frame(0, false);
        }

        if self
            .commands
            .iter()
            .any(|command| matches!(command, Command::SpawnAt(_)))
        {
            self.engine.apply(snapshot);
        }
        self.apply_immediate();

        if !self.started {
            self.started = true;
            self.last_ms = now_ms;
            self.accum = 0.0;
            return self.frame(0, false);
        }

        let dt = now_ms - self.last_ms;
        self.last_ms = now_ms;
        if self.paused || dt < 0.0 {
            self.accum = 0.0;
            return self.frame(0, dt < 0.0);
        }

        self.accum += dt;
        if self.accum < TICK_MS {
            return self.frame(0, false);
        }

        self.apply_deferred();
        let mut ticks = 0;
        while self.accum >= TICK_MS && ticks < MAX_TICKS_PER_FRAME {
            self.accum -= TICK_MS;
            let delta = self.engine.tick(snapshot).clone();
            self.deltas.push(delta);
            let move_from = self.window_moves.len();
            self.engine
                .drain_effects(&mut self.window_moves, &mut self.sounds, &mut self.spawned);
            let move_to = self.window_moves.len();
            for index in move_from..move_to {
                let request = self.window_moves[index];
                self.windows.request_move(&request);
            }
            ticks += 1;
        }
        let resynced = self.accum >= TICK_MS;
        if resynced {
            self.accum = 0.0;
        }
        self.take_diagnostics();
        self.frame(ticks, resynced)
    }

    fn frame(&self, ticks: u32, resynced: bool) -> Frame<'_> {
        Frame {
            ticks,
            resynced,
            deltas: &self.deltas,
            window_moves: &self.window_moves,
            sounds: &self.sounds,
            spawned: &self.spawned,
            diagnostics: &self.diagnostics,
            frozen: &self.frozen,
        }
    }

    fn apply_immediate(&mut self) {
        let mut index = 0;
        while index < self.commands.len() {
            match &self.commands[index] {
                Command::MoveFrozen { .. }
                | Command::Pause(_)
                | Command::SpawnAt(_)
                | Command::SetMascotCap(_) => {
                    let command = self.commands.remove(index);
                    self.dispatch_immediate(command);
                }
                _ => index += 1,
            }
        }
    }

    fn dispatch_immediate(&mut self, command: Command) {
        match command {
            Command::MoveFrozen { id, anchor } => {
                if let Some(delta) = self.engine.move_frozen(id, anchor) {
                    self.frozen.push(delta);
                }
            }
            Command::Pause(flag) => {
                self.paused = flag;
                self.accum = 0.0;
            }
            Command::SpawnAt(anchor) => {
                if self.engine.spawn_at(anchor).is_some() {
                    self.engine.drain_effects(
                        &mut self.window_moves,
                        &mut self.sounds,
                        &mut self.spawned,
                    );
                }
            }
            Command::SetMascotCap(cap) => self.engine.set_mascot_cap(cap),
            _ => {}
        }
    }

    fn apply_deferred(&mut self) {
        for command in self.commands.drain(..) {
            match command {
                Command::Spawn => {
                    self.engine.spawn();
                }
                Command::Dismiss(id) => self.engine.dismiss(id),
                Command::Press { mascot, button } => self.engine.press(mascot, button),
                Command::Release { mascot, button } => self.engine.release(mascot, button),
                Command::Settings(settings) => self.engine.set_settings(settings),
                Command::Window(event) => self.engine.push_window_event(event),
                Command::SetBehavior { id, name } => {
                    let ids: Vec<MascotId> = match id {
                        Some(id) => vec![id],
                        None => self
                            .engine
                            .frames()
                            .into_iter()
                            .map(|frame| frame.id)
                            .collect(),
                    };
                    for id in ids {
                        self.engine.set_behavior(id, &name);
                    }
                }
                Command::SetBehaviorEnabled { name, enabled } => {
                    self.engine.set_behavior_enabled(&name, enabled);
                }
                Command::InterjectBehavior { mascot, behavior } => {
                    self.engine.interject(mascot, &behavior);
                }
                Command::MoveFrozen { .. }
                | Command::Pause(_)
                | Command::SpawnAt(_)
                | Command::SetMascotCap(_) => {}
            }
        }
    }

    fn take_diagnostics(&mut self) {
        let all = self.engine.diagnostics();
        if self.diag_cursor < all.len() {
            self.diagnostics.extend_from_slice(&all[self.diag_cursor..]);
            self.diag_cursor = all.len();
        }
    }
}

fn quiet_pose(character: &Character) -> Option<Pose> {
    let names = [
        "Sit",
        "SitDown",
        "座る",
        "座ってボーっとする",
        "Working",
        "Idle",
        "StandUp",
        "立つ",
    ];
    let named = names
        .iter()
        .filter_map(|name| character.actions.get(name))
        .flat_map(|action| action.animations.iter())
        .flat_map(|animation| animation.poses.iter())
        .find(|pose| {
            pose.velocity_x == 0
                && pose.velocity_y == 0
                && (pose.image_source.is_some() || pose.image.is_some())
        })
        .cloned();
    if named.is_some() {
        return named;
    }
    let mut found = None;
    character.visit_actions(&mut |action| {
        if found.is_none() && action.kind == ActionKind::Stay {
            found = action
                .animations
                .iter()
                .flat_map(|animation| animation.poses.iter())
                .find(|pose| {
                    pose.velocity_x == 0
                        && pose.velocity_y == 0
                        && (pose.image_source.is_some() || pose.image.is_some())
                })
                .cloned();
        }
    });
    found
}

fn fallback_pose(character: &Character) -> Option<Pose> {
    let mut found = None;
    character.visit_actions(&mut |action| {
        if found.is_none() {
            found = action
                .animations
                .iter()
                .flat_map(|animation| animation.poses.iter())
                .find(|pose| pose.image_source.is_some() || pose.image.is_some())
                .cloned();
        }
    });
    found
}

fn park_anchor(anchor: Point, snapshot: &Snapshot) -> Point {
    let Some(rect) = snapshot
        .screens
        .iter()
        .filter(|screen| screen.width > 0 && screen.height > 0)
        .map(screen_rect)
        .min_by_key(|rect| {
            let x = anchor.x.clamp(rect.x, rect.x.saturating_add(rect.width));
            let y = anchor.y.clamp(rect.y, rect.y.saturating_add(rect.height));
            let dx = i128::from(anchor.x) - i128::from(x);
            let dy = i128::from(anchor.y) - i128::from(y);
            dx * dx + dy * dy
        })
    else {
        return anchor;
    };
    let right = rect.x.saturating_add(rect.width);
    let margin = (right.saturating_sub(rect.x) / 4).min(64);
    Point::new(
        anchor
            .x
            .clamp(rect.x.saturating_add(margin), right.saturating_sub(margin)),
        rect.y.saturating_add(rect.height),
    )
}

fn screen_rect(screen: &shimeji_core::env::ScreenSample) -> WorkRect {
    screen
        .work
        .filter(|work| work.width > 0 && work.height > 0)
        .unwrap_or(WorkRect {
            x: screen.x,
            y: screen.y,
            width: screen.width,
            height: screen.height,
        })
}
