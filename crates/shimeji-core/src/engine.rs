//! One engine: the mascot list, the shared [`JavaRandom`], the environment,
//! and the spawn queue.
//!
//! A tick applies queued pointer events, then the environment, then admits
//! spawns and drops removals, then advances each mascot. `total_count` is the
//! live list length after that flush. The condition budget is one counter for
//! the tick; the next tick after a trip starts one mascot later.

use std::collections::HashSet;

use shimeji_expr::JavaRandom;

use crate::behavior::{Catalog, Known, Step};
use crate::caps::{self, DEFAULT_EVAL_BUDGET, DEFAULT_MASCOT_CAP};
use crate::delta::{MascotDelta, MascotId, TickDelta};
use crate::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use crate::env::{Environment, Point, Snapshot};
use crate::manager::{Manager, Slot};
use crate::mascot::{Birth, Mascot};
use crate::model::Character;
use crate::window_control::{self, SoundEvent, WindowEvent, WindowMove};

/// Mouse button. The right button is the popup trigger; the engine ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

/// One mascot after a tick, in manager-list order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MascotFrame {
    pub id: MascotId,
    pub behavior: String,
    pub action: String,
    pub anchor: Point,
    pub image: Option<String>,
    pub look_right: bool,
}

struct RngMeter {
    cursor: JavaRandom,
    draws: u64,
}

impl RngMeter {
    fn new(rng: &JavaRandom) -> Self {
        Self {
            cursor: rng.clone(),
            draws: 0,
        }
    }

    /// Count `next_double` calls since the engine was created.
    ///
    /// `JavaRandom` does not expose its seed. The cursor walks forward until
    /// three peeked draws match the live stream.
    fn sync(&mut self, rng: &JavaRandom) -> u64 {
        for _ in 0..100_000 {
            let mut ahead = self.cursor.clone();
            let mut live = rng.clone();
            let matched =
                (0..3).all(|_| ahead.next_double().to_bits() == live.next_double().to_bits());
            if matched {
                return self.draws;
            }
            self.cursor.next_double();
            self.draws += 1;
        }
        self.draws
    }
}

/// Settings actions read. Scaling is captured by an action's `init`.
/// The other settings are read every tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineSettings {
    pub scaling: f64,
    pub breeding: bool,
    pub transients: bool,
    pub throwing: bool,
    /// Constrain throws to desktop edges. Off by default for reference compatibility.
    pub keep_on_screen: bool,
    /// Ignore window floors/ceilings on the opposite desktop edge. The app
    /// enables this; reference traces keep the original overlapping borders.
    pub desktop_border_priority: bool,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            scaling: 1.0,
            breeding: true,
            transients: true,
            throwing: true,
            keep_on_screen: false,
            desktop_border_priority: false,
        }
    }
}

struct PendingIe {
    id: u64,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

enum Confirm {
    Unreported,
    Moved {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    Failed,
}

/// The runtime a shell drives at 25 Hz.
pub struct Engine {
    catalog: Catalog,
    manager: Manager,
    world: Environment,
    rng: JavaRandom,
    meter: RngMeter,
    disabled: HashSet<String>,
    diags: Vec<Diagnostic>,
    events: Vec<Pointer>,
    /// Pose names queued for the next tick. Empty on every golden run.
    interjects: Vec<(MascotId, String)>,
    cap: usize,
    eval_budget: u32,
    next_id: u64,
    rotation: usize,
    cursor_placed: bool,
    pack: String,
    file: String,
    settings: EngineSettings,
    pending_ie: Option<PendingIe>,
    window_events: Vec<WindowEvent>,
    window_moves: Vec<WindowMove>,
    sound_events: Vec<SoundEvent>,
    spawned_ids: Vec<MascotId>,
    anchor_before: Vec<Point>,
    last_delta: TickDelta,
}

enum Pointer {
    Press {
        mascot: Option<MascotId>,
        button: MouseButton,
    },
    Release {
        mascot: Option<MascotId>,
        button: MouseButton,
    },
}

impl Engine {
    /// Compile `character` and start one RNG at `seed`. No mascot is spawned.
    pub fn new(character: &Character, seed: i64) -> Self {
        let catalog = Catalog::compile(character);
        let diags = catalog.action_diagnostics().to_vec();
        let rng = JavaRandom::new(seed);
        let meter = RngMeter::new(&rng);
        Self {
            catalog,
            manager: Manager::new(),
            world: Environment::new(),
            rng,
            meter,
            disabled: HashSet::new(),
            diags,
            events: Vec::new(),
            interjects: Vec::new(),
            cap: DEFAULT_MASCOT_CAP,
            eval_budget: DEFAULT_EVAL_BUDGET,
            next_id: 1,
            rotation: 0,
            cursor_placed: false,
            pack: character.name.clone(),
            file: character.behaviors_file.clone(),
            settings: EngineSettings::default(),
            pending_ie: None,
            window_events: Vec::new(),
            window_moves: Vec::new(),
            sound_events: Vec::new(),
            spawned_ids: Vec::new(),
            anchor_before: Vec::new(),
            last_delta: TickDelta {
                mascots: Vec::new(),
            },
        }
    }

    pub fn set_settings(&mut self, settings: EngineSettings) {
        self.settings = settings;
    }

    pub fn settings(&self) -> EngineSettings {
        self.settings
    }

    /// Queue a removal. It drops on the next flush, before that tick's behaviors.
    pub fn dismiss(&mut self, id: MascotId) {
        self.manager.dispose(id);
    }

    /// A window-server result from the previous tick's [`WindowMove`].
    pub fn push_window_event(&mut self, event: WindowEvent) {
        self.window_events.push(event);
    }

    /// Append this tick's window moves, sounds, and new mascot ids.
    ///
    /// Both sides keep their buffer capacity, so a steady tick that produces
    /// nothing does not allocate.
    pub fn drain_effects(
        &mut self,
        moves: &mut Vec<WindowMove>,
        sounds: &mut Vec<SoundEvent>,
        spawned: &mut Vec<MascotId>,
    ) {
        moves.append(&mut self.window_moves);
        sounds.append(&mut self.sound_events);
        spawned.append(&mut self.spawned_ids);
    }

    pub fn set_mascot_cap(&mut self, cap: usize) {
        self.cap = caps::clamp_cap(cap);
    }

    pub fn set_eval_budget(&mut self, budget: u32) {
        self.eval_budget = budget;
    }

    /// Disable a toggleable behavior. A behavior that is not toggleable stays enabled.
    pub fn set_behavior_enabled(&mut self, name: &str, enabled: bool) {
        if enabled {
            self.disabled.remove(name);
        } else {
            self.disabled.insert(name.to_string());
        }
    }

    /// `setBehavior` / `buildBehavior(name)` on a live mascot. The anchor stays.
    ///
    /// The reference's Follow Cursor calls this for every mascot of the
    /// character (`setBehaviorAll` of ChaseMouse). A missing name does nothing.
    pub fn set_behavior(&mut self, id: MascotId, name: &str) -> bool {
        if self.manager.slot_mut(id).is_none() {
            return false;
        }
        let built = {
            let Engine {
                catalog,
                manager,
                world,
                rng,
                disabled,
                diags,
                ..
            } = self;
            let Some(slot) = manager.slot_mut(id) else {
                return false;
            };
            catalog.build_checked(name, &mut slot.mascot, world, rng, disabled, diags)
        };
        let Ok(mut active) = built else {
            return false;
        };
        let step = {
            let Engine {
                catalog,
                manager,
                world,
                rng,
                disabled,
                diags,
                ..
            } = self;
            let Some(slot) = manager.slot_mut(id) else {
                return false;
            };
            let step = catalog.enter(&mut active, &mut slot.mascot, world, rng, disabled, diags);
            slot.behavior = active;
            step
        };
        if matches!(step, Step::Dispose) {
            self.manager.dispose(id);
            return false;
        }
        true
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diags
    }

    pub fn population(&self) -> usize {
        self.manager.population()
    }

    /// Cumulative `nextDouble` draws on the shared stream.
    pub fn rng_draws(&mut self) -> u64 {
        self.meter.sync(&self.rng)
    }

    pub fn frames(&self) -> Vec<MascotFrame> {
        self.manager
            .live()
            .iter()
            .map(|slot| MascotFrame {
                id: slot.id,
                behavior: slot.behavior.name.clone(),
                action: slot.behavior.action.active_name().to_string(),
                anchor: slot.mascot.anchor,
                image: slot.mascot.image_key.clone(),
                look_right: slot.mascot.look_right,
            })
            .collect()
    }

    /// `createMascot`: off-screen anchor, one facing draw, then `buildNextBehavior`.
    pub fn spawn(&mut self) -> Option<MascotId> {
        self.create(Point::new(-4000, -4000), None, None)
    }

    /// `spawn`, with the anchor set before the environment sync and the first behavior.
    ///
    /// Tests use this so a floor grab sees the window. It still draws the facing coin-flip.
    pub fn spawn_at(&mut self, anchor: Point) -> Option<MascotId> {
        self.create(anchor, None, None)
    }

    /// `buildBehavior(name, mascot)`. No facing draw. A disabled behavior teleports.
    pub fn spawn_behavior(&mut self, name: &str) -> Option<MascotId> {
        self.create(Point::new(-4000, -4000), Some(false), Some(name))
    }

    /// Publish a snapshot outside a tick. The first call places the cursor.
    pub fn apply(&mut self, snapshot: &Snapshot) {
        self.ingest(snapshot);
    }

    /// Queue a pose for the next tick, before the environment update.
    ///
    /// A behavior name is entered the way a press enters `ドラッグされる`.
    /// Any other action name runs once, then normal selection resumes.
    /// Fall, Dragged, Thrown, and a window carry are left as they are.
    pub fn interject(&mut self, id: MascotId, name: &str) {
        self.interjects.push((id, name.to_string()));
    }

    /// Queue a left or right press. It runs at the next tick, before the environment update.
    pub fn press(&mut self, mascot: Option<MascotId>, button: MouseButton) {
        self.events.push(Pointer::Press { mascot, button });
    }

    /// Queue a release. A same-tick press has not set `dragging` yet, so it does not throw.
    pub fn release(&mut self, mascot: Option<MascotId>, button: MouseButton) {
        self.events.push(Pointer::Release { mascot, button });
    }

    /// Move a paused pet. The anchor changes immediately, outside the tick.
    pub fn move_frozen(&mut self, id: MascotId, anchor: Point) -> Option<MascotDelta> {
        let slot = self.manager.slot_any_mut(id)?;
        let before = slot.mascot.anchor;
        slot.mascot.anchor = anchor;
        Some(MascotDelta {
            id,
            anchor_changed: before != anchor,
            image_changed: false,
        })
    }

    /// Frequencies the next selection would weight for this mascot's current behavior.
    ///
    /// Evaluates conditions, so a condition that calls `Math.random` draws.
    pub fn candidate_weights(&mut self, id: MascotId) -> Option<Vec<(String, i32)>> {
        let name = self.manager.behavior_name(id)?.to_string();
        let Engine {
            catalog,
            manager,
            rng,
            disabled,
            diags,
            ..
        } = self;
        let slot = manager.slot_any_mut(id)?;
        Some(catalog.candidate_weights(Some(&name), &slot.mascot, rng, disabled, diags))
    }

    /// One tick. Pointer events, then the environment, then the mascots.
    ///
    /// The returned delta is borrowed from the engine. Copy the flags out
    /// before the next `tick` if a caller needs both.
    pub fn tick(&mut self, snapshot: &Snapshot) -> &TickDelta {
        let confirm = self.take_confirm();
        self.drain_pointer();
        self.drain_interject();
        self.ingest(snapshot);
        self.reconcile(confirm);
        self.manager.flush();
        let count = self.manager.live_len() as i32;
        for slot in self.manager.live_mut() {
            slot.mascot.total_count = count;
        }

        let n = self.manager.live_len();
        let start = if n == 0 { 0 } else { self.rotation % n };
        self.anchor_before.clear();
        self.anchor_before
            .extend(self.manager.live().iter().map(|slot| slot.mascot.anchor));

        let mut left = self.eval_budget;
        let mut tripped = false;
        for step in 0..n {
            let index = (start + step) % n;
            let outcome = self.tick_mascot(index, left);
            left = outcome.left;
            tripped |= outcome.tripped;
        }
        if tripped {
            if n > 0 {
                self.rotation = (start + 1) % n;
            }
            self.budget_diagnostic();
        }

        self.last_delta.mascots.clear();
        for (index, slot) in self.manager.live_mut().iter_mut().enumerate() {
            let anchor_changed = self
                .anchor_before
                .get(index)
                .is_some_and(|before| *before != slot.mascot.anchor);
            let image_changed = slot.mascot.image_dirty;
            slot.mascot.image_dirty = false;
            self.last_delta.mascots.push(MascotDelta {
                id: slot.id,
                anchor_changed,
                image_changed,
            });
        }
        &self.last_delta
    }

    fn create(
        &mut self,
        anchor: Point,
        facing: Option<bool>,
        named: Option<&str>,
    ) -> Option<MascotId> {
        if self.manager.population() >= self.cap {
            self.diags.push(Diagnostic::new(
                Severity::Error,
                DiagnosticKind::Other,
                &self.pack,
                &self.file,
                None,
                "mascot cap reached; spawn refused",
            ));
            return None;
        }
        let mut mascot = Mascot::new();
        mascot.anchor = anchor;
        mascot.total_count = self.manager.live_len() as i32;
        self.copy_settings(&mut mascot);
        mascot.sync(&self.world, true);
        match facing {
            None => mascot.look_right = self.rng.next_double() < 0.5,
            Some(look_right) => mascot.look_right = look_right,
        }
        mascot.tick_budget.set_left(self.eval_budget);
        let built = match named {
            Some(name) => self.catalog.build_checked(
                name,
                &mut mascot,
                &self.world,
                &mut self.rng,
                &self.disabled,
                &mut self.diags,
            ),
            None => self.catalog.build_next(
                None,
                &mut mascot,
                &self.world,
                &mut self.rng,
                &self.disabled,
                &mut self.diags,
            ),
        };
        let mut active = match built {
            Ok(active) => active,
            Err(()) => return None,
        };
        let step = self.catalog.enter(
            &mut active,
            &mut mascot,
            &self.world,
            &mut self.rng,
            &self.disabled,
            &mut self.diags,
        );
        if mascot.tick_budget.tripped() {
            self.budget_diagnostic();
        }
        if matches!(step, Step::Dispose) {
            return None;
        }
        let id = MascotId(self.next_id);
        self.next_id += 1;
        self.manager.enqueue(Slot {
            id,
            mascot,
            behavior: active,
        });
        self.spawned_ids.push(id);
        Some(id)
    }

    fn copy_settings(&self, mascot: &mut Mascot) {
        mascot.scaling = self.settings.scaling;
        mascot.breeding = self.settings.breeding;
        mascot.transients = self.settings.transients;
        mascot.throwing = self.settings.throwing;
        mascot.keep_on_screen = self.settings.keep_on_screen;
        mascot.env.desktop_border_priority = self.settings.desktop_border_priority;
    }

    fn tick_mascot(&mut self, index: usize, left: u32) -> TickOutcome {
        let id = self.manager.live()[index].id;
        {
            let room = self.cap.saturating_sub(self.manager.population());
            let settings = self.settings;
            let mascot = &mut self.manager.live_mut()[index].mascot;
            mascot.scaling = settings.scaling;
            mascot.breeding = settings.breeding;
            mascot.transients = settings.transients;
            mascot.throwing = settings.throwing;
            mascot.keep_on_screen = settings.keep_on_screen;
            mascot.env.desktop_border_priority = settings.desktop_border_priority;
            mascot.spawn_room = room;
        }
        self.manager.live_mut()[index]
            .mascot
            .tick_budget
            .set_left(left);
        self.manager.live_mut()[index]
            .mascot
            .tick_budget
            .clear_tripped();
        let step = {
            let Engine {
                catalog,
                manager,
                world,
                rng,
                disabled,
                diags,
                ..
            } = self;
            let slot = &mut manager.live_mut()[index];
            let force = slot.behavior.action.class_name() == "Dragged";
            slot.mascot.sync(world, force);
            catalog.next(
                &mut slot.behavior,
                &mut slot.mascot,
                world,
                rng,
                disabled,
                diags,
            )
        };
        if matches!(step, Step::Dispose) {
            self.manager.dispose(id);
        }
        self.commit_ie(index);
        self.take_sounds(index);
        self.hatch(index);
        let mascot = &mut self.manager.live_mut()[index].mascot;
        mascot.time = mascot.time.wrapping_add(1);
        TickOutcome {
            left: mascot.tick_budget.left(),
            tripped: mascot.tick_budget.tripped(),
        }
    }

    fn drain_interject(&mut self) {
        if self.interjects.is_empty() {
            return;
        }
        let queued = std::mem::take(&mut self.interjects);
        for (id, name) in queued {
            self.apply_interject(id, &name);
        }
    }

    fn apply_interject(&mut self, id: MascotId, name: &str) {
        if self.interject_blocked(id) {
            return;
        }
        let built = if self.catalog.contains_behavior(name) {
            self.catalog.build_named(name, &mut self.diags)
        } else {
            self.catalog.build_action_once(name, &mut self.diags)
        };
        let Ok(mut active) = built else {
            return;
        };
        let step = {
            let Engine {
                catalog,
                manager,
                world,
                rng,
                disabled,
                diags,
                ..
            } = self;
            let Some(slot) = manager.slot_mut(id) else {
                return;
            };
            let step = catalog.enter(&mut active, &mut slot.mascot, world, rng, disabled, diags);
            slot.behavior = active;
            step
        };
        if matches!(step, Step::Dispose) {
            self.manager.dispose(id);
        }
    }

    fn interject_blocked(&mut self, id: MascotId) -> bool {
        let behavior_name = {
            let Some(slot) = self.manager.slot_mut(id) else {
                return true;
            };
            if slot.behavior.action.blocks_pose_interject() {
                return true;
            }
            slot.behavior.name.clone()
        };
        behavior_name == self.catalog.behavior_name(Known::Fall)
            || behavior_name == self.catalog.behavior_name(Known::Dragged)
            || behavior_name == self.catalog.behavior_name(Known::Thrown)
    }

    fn drain_pointer(&mut self) {
        let events = std::mem::take(&mut self.events);
        for event in events {
            match event {
                Pointer::Press {
                    mascot,
                    button: MouseButton::Left,
                } => self.apply_press(mascot),
                Pointer::Release {
                    mascot,
                    button: MouseButton::Left,
                } => self.apply_release(mascot),
                Pointer::Press {
                    button: MouseButton::Right,
                    ..
                }
                | Pointer::Release {
                    button: MouseButton::Right,
                    ..
                } => {}
            }
        }
    }

    fn apply_press(&mut self, mascot: Option<MascotId>) {
        let Some(id) = self.resolve_live(mascot) else {
            return;
        };
        let draggable = {
            let Engine { manager, rng, .. } = self;
            let Some(slot) = manager.slot_mut(id) else {
                return;
            };
            slot.behavior.action.draggable(&slot.mascot, rng)
        };
        match draggable {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                self.catalog.report(&error, &mut self.diags);
                self.manager.dispose(id);
                return;
            }
        }
        let built = self.catalog.build_known(Known::Dragged, &mut self.diags);
        let Ok(mut active) = built else {
            self.manager.dispose(id);
            return;
        };
        let step = {
            let Engine {
                catalog,
                manager,
                world,
                rng,
                disabled,
                diags,
                ..
            } = self;
            let Some(slot) = manager.slot_mut(id) else {
                return;
            };
            let step = catalog.enter(&mut active, &mut slot.mascot, world, rng, disabled, diags);
            slot.behavior = active;
            step
        };
        if matches!(step, Step::Dispose) {
            self.manager.dispose(id);
        }
    }

    fn apply_release(&mut self, mascot: Option<MascotId>) {
        let Some(id) = self.resolve_live(mascot) else {
            return;
        };
        let dragging = self.manager.slot_mut(id).is_some_and(|slot| {
            let dragging = slot.mascot.dragging;
            if dragging {
                slot.mascot.dragging = false;
            }
            dragging
        });
        if !dragging {
            return;
        }
        let built = self.catalog.build_known(Known::Thrown, &mut self.diags);
        let Ok(mut active) = built else {
            self.manager.dispose(id);
            return;
        };
        let step = {
            let Engine {
                catalog,
                manager,
                world,
                rng,
                disabled,
                diags,
                ..
            } = self;
            let Some(slot) = manager.slot_mut(id) else {
                return;
            };
            let step = catalog.enter(&mut active, &mut slot.mascot, world, rng, disabled, diags);
            slot.behavior = active;
            step
        };
        if matches!(step, Step::Dispose) {
            self.manager.dispose(id);
        }
    }

    fn resolve_live(&self, mascot: Option<MascotId>) -> Option<MascotId> {
        match mascot {
            Some(id) if self.manager.contains_live(id) => Some(id),
            Some(_) => None,
            None => self.manager.first_live(),
        }
    }

    fn ingest(&mut self, snapshot: &Snapshot) {
        let cursor = snapshot.cursor;
        self.world.apply(snapshot);
        if !self.cursor_placed {
            self.world.place_cursor(cursor.x, cursor.y);
            self.cursor_placed = true;
        }
    }

    fn commit_ie(&mut self, index: usize) {
        let Some((x, y)) = self.manager.live_mut()[index].mascot.ie_move.take() else {
            return;
        };
        let ie = *self.manager.live()[index].mascot.env.active_ie();
        let id = self.manager.live()[index].mascot.env.active_id();
        let width = ie.width();
        let height = ie.height();
        self.world.place_active_ie(x, y, width, height);
        self.pending_ie = Some(PendingIe {
            id,
            x,
            y,
            width,
            height,
        });
        self.window_moves.push(WindowMove {
            id,
            x,
            y,
            width,
            height,
        });
    }

    fn take_sounds(&mut self, index: usize) {
        let id = self.manager.live()[index].id;
        if self.manager.live()[index].mascot.sounds.is_empty() {
            return;
        }
        let sounds = std::mem::take(&mut self.manager.live_mut()[index].mascot.sounds);
        self.sound_events
            .extend(sounds.into_iter().map(|(name, volume)| SoundEvent {
                mascot: id,
                name,
                volume,
            }));
    }

    fn hatch(&mut self, index: usize) {
        let births = std::mem::take(&mut self.manager.live_mut()[index].mascot.births);
        for Birth {
            anchor,
            look_right,
            behavior,
        } in births
        {
            if behavior.is_empty() {
                continue;
            }
            self.create(anchor, Some(look_right), Some(behavior.as_str()));
        }
    }

    fn take_confirm(&mut self) -> Confirm {
        let mut confirm = Confirm::Unreported;
        for event in self.window_events.drain(..) {
            confirm = match event {
                WindowEvent::Failed { id } => {
                    if self
                        .pending_ie
                        .as_ref()
                        .is_some_and(|pending| pending.id == id)
                    {
                        Confirm::Failed
                    } else {
                        confirm
                    }
                }
                WindowEvent::Moved {
                    id,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if self
                        .pending_ie
                        .as_ref()
                        .is_some_and(|pending| pending.id == id)
                    {
                        Confirm::Moved {
                            x,
                            y,
                            width,
                            height,
                        }
                    } else {
                        confirm
                    }
                }
            };
        }
        confirm
    }

    /// A carry survives when the window is within [`CARRY_TOLERANCE_PX`] of the request.
    ///
    /// The believed rectangle is written back so the corner test stays exact and
    /// the snapshot's correction is not a second resize. Anything further ends
    /// the carry and drops the snap-back delta.
    fn reconcile(&mut self, confirm: Confirm) {
        let Some(pending) = self.pending_ie.take() else {
            return;
        };
        match confirm {
            Confirm::Failed => self.world.clear_active_deltas(),
            Confirm::Moved {
                x,
                y,
                width,
                height,
            } => {
                if near_ie(&pending, x, y, width, height) {
                    self.world
                        .place_active_ie(pending.x, pending.y, pending.width, pending.height);
                } else {
                    self.world.place_active_ie(x, y, width, height);
                }
            }
            Confirm::Unreported => {
                let ie = *self.world.active_ie();
                let same = ie.visible && self.world.active_id() == pending.id;
                if same && near_ie(&pending, ie.left, ie.top, ie.width(), ie.height()) {
                    self.world
                        .place_active_ie(pending.x, pending.y, pending.width, pending.height);
                } else {
                    self.world.clear_active_deltas();
                }
            }
        }
    }

    fn budget_diagnostic(&mut self) {
        self.diags.push(Diagnostic::new(
            Severity::Warning,
            DiagnosticKind::Other,
            &self.pack,
            &self.file,
            None,
            "evaluation budget exhausted; remaining conditions are false",
        ));
    }
}

struct TickOutcome {
    left: u32,
    tripped: bool,
}

fn near_ie(pending: &PendingIe, x: i32, y: i32, width: i32, height: i32) -> bool {
    let limit = window_control::CARRY_TOLERANCE_PX as u32;
    pending.x.wrapping_sub(x).unsigned_abs() <= limit
        && pending.y.wrapping_sub(y).unsigned_abs() <= limit
        && pending.width.wrapping_sub(width).unsigned_abs() <= limit
        && pending.height.wrapping_sub(height).unsigned_abs() <= limit
}
