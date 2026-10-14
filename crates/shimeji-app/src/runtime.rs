//! Fixed-step clock the macOS shell drives.
//!
//! Timestamps are milliseconds. The step is 40 ms. One call runs at most two
//! ticks, then drops the leftover so a stall does not replay a backlog.
//! [`Command::MoveFrozen`], [`Command::Pause`], [`Command::SpawnAt`], and
//! [`Command::SetMascotCap`] apply as they arrive. Every other command waits
//! for a tick boundary and runs once, before the burst.

use shimeji_core::diagnostics::Diagnostic;
use shimeji_core::env::{Point, Snapshot};
use shimeji_core::model::Character;
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
        self.engine.frames()
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
