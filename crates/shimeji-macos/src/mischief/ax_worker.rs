//! One serial worker for every Accessibility call, plus the pure queue in
//! front of it.
//!
//! Moves for one window collapse to the latest destination. The first origin
//! and pid stay, because that origin is what the journal restores to. The
//! worker never runs on the main thread. A move that is still inside the
//! backend after [`MOVE_TIMEOUT`] is failed from the frame side: this thread
//! cannot notice the deadline while it is blocked in the call.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use shimeji_app::journal::{
    AxWindow, Bounds, CarryRefusal, EndSpec, LaunchId, LiveWindows, LoadError, Operation,
    RestoreMove, SlotAllocator, StartSpec, assess_carry, clamp_partly_on_screen,
    clear_journal_files, restore_directory,
};
use shimeji_app::window_rules::WindowRules;
use shimeji_core::window_control::WindowEvent;

/// A move the engine asked for. `origin` is the window before this carry's
/// first request. Later requests replace `dest` only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMove {
    pub id: u64,
    pub pid: i32,
    pub window: u32,
    pub origin: Bounds,
    pub dest: Bounds,
}

/// Latest-wins queue. Insertion order is the order carries are attempted.
#[derive(Debug, Default)]
pub struct MoveQueue {
    order: VecDeque<u64>,
    map: HashMap<u64, PendingMove>,
}

impl MoveQueue {
    pub fn push(&mut self, pending: PendingMove) {
        if let Some(existing) = self.map.get_mut(&pending.id) {
            existing.dest = pending.dest;
            return;
        }
        self.order.push_back(pending.id);
        self.map.insert(pending.id, pending);
    }

    pub fn contains(&self, id: u64) -> bool {
        self.map.contains_key(&id)
    }

    pub fn next_ready(&self, mut ready: impl FnMut(u64) -> bool) -> Option<u64> {
        self.order.iter().copied().find(|id| ready(*id))
    }

    pub fn take(&mut self, id: u64) -> Option<PendingMove> {
        let pending = self.map.remove(&id)?;
        self.order.retain(|queued| *queued != id);
        Some(pending)
    }

    pub fn remove(&mut self, id: u64) {
        let _ = self.take(id);
    }
}

/// How long a move may sit inside the backend before the frame fails it.
pub const MOVE_TIMEOUT: Duration = Duration::from_millis(100);

struct InFlight {
    id: u64,
    epoch: u64,
    started: Instant,
}

/// The frame and the worker share this. The mutex is not held across the move.
pub struct Flight {
    epoch: AtomicU64,
    inner: Mutex<Option<InFlight>>,
}

impl Flight {
    pub fn new() -> Self {
        Self {
            epoch: AtomicU64::new(0),
            inner: Mutex::new(None),
        }
    }

    pub fn begin(&self, id: u64) -> u64 {
        let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        *lock(&self.inner) = Some(InFlight {
            id,
            epoch,
            started: Instant::now(),
        });
        epoch
    }

    pub fn current(&self, epoch: u64) -> bool {
        self.epoch.load(Ordering::Acquire) == epoch
    }

    /// Invalidate a move that has been inside the backend for `limit`.
    pub fn timed_out(&self, limit: Duration) -> Option<u64> {
        let mut guard = lock(&self.inner);
        let state = guard.as_ref()?;
        if state.started.elapsed() < limit {
            return None;
        }
        let id = state.id;
        self.epoch.fetch_add(1, Ordering::AcqRel);
        *guard = None;
        Some(id)
    }

    pub fn finish(&self, epoch: u64) {
        let mut guard = lock(&self.inner);
        if guard.as_ref().is_some_and(|state| state.epoch == epoch) {
            *guard = None;
        }
    }
}

impl Default for Flight {
    fn default() -> Self {
        Self::new()
    }
}

/// What the shell can see of the frontmost window, copied off the main thread.
#[derive(Debug, Clone)]
pub struct TargetView {
    pub pid: i32,
    pub window: u64,
    pub bounds: Bounds,
    pub screens: Vec<Bounds>,
    pub rules: WindowRules,
    pub titles: bool,
}

impl Default for TargetView {
    fn default() -> Self {
        Self {
            pid: 0,
            window: 0,
            bounds: Bounds {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            screens: Vec::new(),
            rules: WindowRules::default(),
            titles: false,
        }
    }
}

/// The backend could not place the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveMiss;

/// AX calls the worker is allowed to make. Tests supply a fake.
pub trait AxBackend: Send {
    fn prepare(&self) {}
    fn windows(&self, pid: i32, with_title: bool) -> Vec<AxWindow>;
    fn app_name(&self, pid: i32) -> Option<String>;
    fn pid_alive(&self, pid: i32) -> bool;
    fn cg_bounds(&self, pid: i32, window: u32) -> Option<Bounds>;
    fn move_matched(&self, index: usize, bounds: Bounds) -> Result<Bounds, MoveMiss>;
}

/// Journal operations the worker issues. The real one forwards to the journal
/// thread; tests answer the gate immediately.
pub trait JournalPort: Send {
    fn request_begin(&mut self, id: u64, spec: StartSpec);
    fn poll_begin(&mut self, id: u64) -> Option<Result<(), CarryRefusal>>;
    fn write_end(&mut self, spec: &EndSpec);
    fn write_applied(&mut self, slot: u16, pid: i32, window: u32, bounds: Bounds);
    fn write_header(&mut self, launch: LaunchId);
    fn release_files(&mut self);
    fn flush(&mut self);
}

/// Carry the worker is tracking. The slot is assigned on the first request
/// and kept for the session by [`SlotAllocator`].
#[derive(Debug)]
struct Tracked {
    spec: StartSpec,
    current: Bounds,
    holding: bool,
    cancelled: bool,
}

/// First request of a window waits for the journal gate. Later requests apply.
#[derive(Debug)]
pub struct CarryTracker {
    slots: SlotAllocator,
    by_id: HashMap<u64, Tracked>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Call `begin_carry` and do not move until [`CarryTracker::allow`].
    Hold(StartSpec),
    /// The gate has not answered.
    Waiting,
    Apply {
        slot: u16,
    },
    Refuse,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Ended {
    pub end: EndSpec,
    pub origin: Bounds,
}

impl CarryTracker {
    pub fn new() -> Self {
        Self {
            slots: SlotAllocator::new(),
            by_id: HashMap::new(),
        }
    }

    pub fn consider(
        &mut self,
        id: u64,
        pid: i32,
        window: u32,
        origin: Bounds,
        now_ms: u64,
    ) -> Decision {
        if let Some(carry) = self.by_id.get(&id) {
            if carry.holding {
                return Decision::Waiting;
            }
            return Decision::Apply {
                slot: carry.spec.slot,
            };
        }
        let Ok(slot) = self.slots.assign(pid, window) else {
            return Decision::Refuse;
        };
        let spec = StartSpec {
            op: Operation::Carry,
            pid,
            window,
            slot,
            bounds: origin,
            at_ms: now_ms,
        };
        self.by_id.insert(
            id,
            Tracked {
                spec,
                current: origin,
                holding: true,
                cancelled: false,
            },
        );
        Decision::Hold(spec)
    }

    /// The gate accepted the start. A carry cancelled while holding ends
    /// without a move: the start line is already durable.
    pub fn allow(&mut self, id: u64, now_ms: u64) -> Option<EndSpec> {
        let carry = self.by_id.get_mut(&id)?;
        if !carry.holding {
            return None;
        }
        carry.holding = false;
        if !carry.cancelled {
            return None;
        }
        let spec = self.by_id.remove(&id)?.spec;
        Some(end_spec(&spec, now_ms))
    }

    pub fn refuse(&mut self, id: u64) {
        if self.by_id.get(&id).is_some_and(|carry| carry.holding) {
            self.by_id.remove(&id);
        }
    }

    pub fn note_applied(&mut self, id: u64, bounds: Bounds) {
        if let Some(carry) = self.by_id.get_mut(&id) {
            if !carry.holding {
                carry.current = bounds;
            }
        }
    }

    pub fn current(&self, id: u64) -> Option<Bounds> {
        self.by_id.get(&id).map(|carry| carry.current)
    }

    pub fn holding(&self, id: u64) -> bool {
        self.by_id.get(&id).is_some_and(|carry| carry.holding)
    }

    pub fn holding_ids(&self) -> Vec<u64> {
        self.by_id
            .iter()
            .filter(|(_, carry)| carry.holding)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn any_holding(&self) -> bool {
        self.by_id.values().any(|carry| carry.holding)
    }

    /// End an open carry. A hold stays a hold, marked cancelled.
    pub fn finish(&mut self, id: u64, now_ms: u64) -> Option<Ended> {
        let carry = self.by_id.get_mut(&id)?;
        if carry.holding {
            carry.cancelled = true;
            return None;
        }
        let carry = self.by_id.remove(&id)?;
        Some(Ended {
            end: end_spec(&carry.spec, now_ms),
            origin: carry.spec.bounds,
        })
    }

    /// End carries the runtime has released, not merely windows without a move.
    pub fn finish_inactive(&mut self, active: &[u64], now_ms: u64) -> Vec<Ended> {
        let ids: Vec<u64> = self
            .by_id
            .iter()
            .filter(|(id, _)| !active.contains(id))
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.finish(id, now_ms))
            .collect()
    }

    /// Permission lost or quit. Open carries end now. Holds end when the gate answers.
    pub fn finish_all(&mut self, now_ms: u64) -> Vec<Ended> {
        let ids: Vec<u64> = self.by_id.keys().copied().collect();
        let mut ended = Vec::new();
        for id in ids {
            if let Some(item) = self.finish(id, now_ms) {
                ended.push(item);
            }
        }
        ended
    }
}

impl Default for CarryTracker {
    fn default() -> Self {
        Self::new()
    }
}

fn end_spec(spec: &StartSpec, now_ms: u64) -> EndSpec {
    EndSpec {
        op: spec.op,
        pid: spec.pid,
        window: spec.window,
        slot: spec.slot,
        at_ms: now_ms,
    }
}

pub enum Cmd {
    Move(PendingMove),
    /// Window ids a ticked runtime still asked to move.
    Activity(Vec<u64>),
    EndAll,
    LaunchRestore(LaunchId),
    TimedOut(u64),
    Shutdown(Sender<()>),
    Stop,
}

pub(crate) struct Shared {
    pub events: Arc<Mutex<Vec<WindowEvent>>>,
    pub flight: Arc<Flight>,
    pub live: Arc<AtomicBool>,
    pub target: Arc<Mutex<TargetView>>,
    pub booted: Arc<AtomicBool>,
    pub stop: Arc<AtomicBool>,
}

enum Pace {
    /// Run again without waiting.
    Progress,
    /// A gate is in flight, or nothing is queued.
    Wait(Duration),
    Stop,
}

pub(crate) fn run<B: AxBackend, J: JournalPort>(
    rx: Receiver<Cmd>,
    backend: B,
    journal: J,
    shared: Shared,
    dir: PathBuf,
) {
    backend.prepare();
    let mut worker = Worker {
        rx,
        inbox: VecDeque::new(),
        backend,
        journal,
        shared,
        carries: CarryTracker::new(),
        queue: MoveQueue::default(),
        dir,
    };
    loop {
        if worker.shared.stop.load(Ordering::Acquire) {
            break;
        }
        match worker.step() {
            Pace::Stop => break,
            Pace::Progress => {}
            Pace::Wait(limit) => {
                // Leave the command in the inbox. `recv_timeout` would take it
                // out of the channel and the next `pull` would never see it.
                if let Ok(cmd) = worker.rx.recv_timeout(limit) {
                    worker.inbox.push_back(cmd);
                }
            }
        }
    }
}

struct Worker<B, J> {
    rx: Receiver<Cmd>,
    inbox: VecDeque<Cmd>,
    backend: B,
    journal: J,
    shared: Shared,
    carries: CarryTracker,
    queue: MoveQueue,
    dir: PathBuf,
}

impl<B: AxBackend, J: JournalPort> Worker<B, J> {
    fn step(&mut self) -> Pace {
        self.pull();
        let mut progress = false;
        while let Some(cmd) = self.inbox.pop_front() {
            if let Cmd::Move(pending) = cmd {
                self.queue.push(pending);
                progress = true;
                continue;
            }
            match self.command(cmd) {
                Pace::Stop => return Pace::Stop,
                Pace::Progress => progress = true,
                Pace::Wait(_) => {}
            }
        }
        if self.poll_gates() {
            progress = true;
        }
        if self.apply_one() {
            progress = true;
        }
        if self.shared.stop.load(Ordering::Acquire) {
            return Pace::Stop;
        }
        if self.carries.any_holding() {
            return Pace::Wait(Duration::from_millis(5));
        }
        if progress {
            Pace::Progress
        } else {
            Pace::Wait(Duration::from_millis(20))
        }
    }

    fn pull(&mut self) {
        while let Ok(cmd) = self.rx.try_recv() {
            self.inbox.push_back(cmd);
        }
    }

    fn command(&mut self, cmd: Cmd) -> Pace {
        match cmd {
            Cmd::Move(pending) => {
                self.queue.push(pending);
                Pace::Progress
            }
            Cmd::Activity(ids) => {
                for ended in self.carries.finish_inactive(&ids, unix_ms()) {
                    self.journal.write_end(&ended.end);
                    self.queue.remove(ended.end.window as u64);
                }
                Pace::Progress
            }
            Cmd::TimedOut(id) => {
                if let Some(ended) = self.carries.finish(id, unix_ms()) {
                    self.journal.write_end(&ended.end);
                    self.queue.remove(id);
                }
                Pace::Progress
            }
            Cmd::EndAll => {
                self.restore_open(false);
                Pace::Progress
            }
            Cmd::LaunchRestore(launch) => {
                self.launch_restore(launch);
                self.shared.booted.store(true, Ordering::Release);
                Pace::Progress
            }
            Cmd::Shutdown(ack) => {
                self.restore_open(true);
                let _ = ack.send(());
                Pace::Stop
            }
            Cmd::Stop => Pace::Stop,
        }
    }

    fn poll_gates(&mut self) -> bool {
        let ids = self.carries.holding_ids();
        let mut progress = false;
        for id in ids {
            let Some(answer) = self.journal.poll_begin(id) else {
                continue;
            };
            progress = true;
            match answer {
                Ok(()) => {
                    if let Some(end) = self.carries.allow(id, unix_ms()) {
                        self.journal.write_end(&end);
                        self.queue.remove(id);
                    }
                }
                Err(_) => {
                    self.carries.refuse(id);
                    self.queue.remove(id);
                    self.fail(id);
                }
            }
        }
        progress
    }

    fn apply_one(&mut self) -> bool {
        let Some(id) = self.queue.next_ready(|id| !self.carries.holding(id)) else {
            return false;
        };
        let Some(pending) = self.queue.take(id) else {
            return false;
        };
        match self
            .carries
            .consider(id, pending.pid, pending.window, pending.origin, unix_ms())
        {
            Decision::Refuse => self.fail(id),
            Decision::Waiting => self.queue.push(pending),
            Decision::Hold(spec) => {
                self.journal.request_begin(id, spec);
                self.queue.push(pending);
            }
            Decision::Apply { slot } => self.apply_move(pending, slot),
        }
        true
    }

    fn apply_move(&mut self, pending: PendingMove, slot: u16) {
        let target = lock(&self.shared.target).clone();
        let matched = self.matched(&pending, &target);
        let Some(index) = matched else {
            self.fail(pending.id);
            self.fail_carry(pending.id);
            return;
        };
        let Some(dest) = clamp_partly_on_screen(pending.dest, &target.screens) else {
            self.fail(pending.id);
            self.fail_carry(pending.id);
            return;
        };
        let epoch = self.shared.flight.begin(pending.id);
        let moved = self.backend.move_matched(index, dest);
        let still = self.shared.flight.current(epoch);
        self.shared.flight.finish(epoch);
        self.pull();
        self.absorb_moves();
        match moved {
            Ok(bounds) => {
                self.journal
                    .write_applied(slot, pending.pid, pending.window, bounds);
                self.carries.note_applied(pending.id, bounds);
                let newer = self.queue.contains(pending.id);
                if still && !newer {
                    self.emit(WindowEvent::Moved {
                        id: pending.id,
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: bounds.height,
                    });
                } else if !still {
                    self.fail_carry(pending.id);
                }
            }
            Err(MoveMiss) => {
                if still {
                    self.fail(pending.id);
                }
                self.fail_carry(pending.id);
            }
        }
    }

    fn matched(&mut self, pending: &PendingMove, target: &TargetView) -> Option<usize> {
        let cg = self.carries.current(pending.id).unwrap_or(pending.origin);
        let windows = self.backend.windows(pending.pid, target.titles);
        let app = self.backend.app_name(pending.pid).unwrap_or_default();
        let result = assess_carry(&target.rules, &app, cg, &windows);
        if std::env::var_os("SHIMEJI_TRACE_MOTION").is_some() {
            eprintln!(
                "mischief_match pid={} window={} app={app:?} cg={cg:?} result={result:?} ax={:?}",
                pending.pid,
                pending.window,
                windows
                    .iter()
                    .map(|w| (&w.bounds, &w.subrole, w.position_settable))
                    .collect::<Vec<_>>()
            );
        }
        result.ok()
    }

    fn absorb_moves(&mut self) {
        let mut rest = VecDeque::new();
        while let Some(cmd) = self.inbox.pop_front() {
            if let Cmd::Move(pending) = cmd {
                self.queue.push(pending);
            } else {
                rest.push_back(cmd);
            }
        }
        self.inbox = rest;
    }

    fn fail_carry(&mut self, id: u64) {
        if let Some(ended) = self.carries.finish(id, unix_ms()) {
            self.journal.write_end(&ended.end);
            self.queue.remove(id);
        }
    }

    fn fail(&self, id: u64) {
        self.emit(WindowEvent::Failed { id });
    }

    fn emit(&self, event: WindowEvent) {
        lock(&self.shared.events).push(event);
    }

    fn restore_open(&mut self, _quit: bool) {
        for ended in self.carries.finish_all(unix_ms()) {
            self.journal.write_end(&ended.end);
        }
        self.journal.flush();
        self.shared.live.store(false, Ordering::Release);
        if self.restore_now() == RestoreOutcome::Applied {
            self.clear_journal();
        }
    }

    fn launch_restore(&mut self, launch: LaunchId) {
        self.shared.live.store(false, Ordering::Release);
        if self.restore_now() != RestoreOutcome::Applied {
            return;
        }
        if !self.clear_journal() {
            return;
        }
        self.journal.write_header(launch);
        self.journal.flush();
        if !self.shared.stop.load(Ordering::Acquire) {
            self.shared.live.store(true, Ordering::Release);
        }
    }

    fn restore_now(&mut self) -> RestoreOutcome {
        let target = lock(&self.shared.target).clone();
        restore_session(
            &self.backend,
            &self.shared.stop,
            &self.dir,
            &target.rules,
            &target.screens,
            target.titles,
            unix_ms(),
        )
    }

    fn clear_journal(&mut self) -> bool {
        self.journal.release_files();
        self.journal.flush();
        match clear_journal_files(&self.dir) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("mischief_journal={error}");
                false
            }
        }
    }
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

/// The worker needs the profile directory. It is stored beside the backend.
pub fn restore_session<B: AxBackend>(
    backend: &B,
    stop: &AtomicBool,
    dir: &Path,
    rules: &WindowRules,
    screens: &[Bounds],
    titles: bool,
    now_ms: u64,
) -> RestoreOutcome {
    let live = BackendWindows {
        backend,
        screens,
        titles,
    };
    match restore_directory(dir, rules, &live, now_ms) {
        Err(LoadError::TooLarge(bytes)) => {
            println!("mischief_restore=too_large bytes={bytes}");
            RestoreOutcome::LeftInPlace
        }
        Err(LoadError::Io(error)) => {
            println!("mischief_restore=io {error}");
            RestoreOutcome::LeftInPlace
        }
        Ok(plan) => {
            println!(
                "mischief_restore=moves={} skipped={}",
                plan.moves.len(),
                plan.skipped.len()
            );
            let mut applied = 0;
            for movement in &plan.moves {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if apply_restore(backend, movement, rules, screens, titles) {
                    applied += 1;
                }
            }
            if applied == plan.moves.len() {
                RestoreOutcome::Applied
            } else {
                println!("mischief_restore=partial applied={applied}");
                RestoreOutcome::LeftInPlace
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreOutcome {
    Applied,
    LeftInPlace,
}

fn apply_restore<B: AxBackend>(
    backend: &B,
    movement: &RestoreMove,
    rules: &WindowRules,
    screens: &[Bounds],
    titles: bool,
) -> bool {
    let Some(current) = backend.cg_bounds(movement.pid, movement.window) else {
        return false;
    };
    let windows = backend.windows(movement.pid, titles);
    let app = backend.app_name(movement.pid).unwrap_or_default();
    let Ok(index) = assess_carry(rules, &app, current, &windows) else {
        return false;
    };
    let Some(dest) = clamp_partly_on_screen(movement.to, screens) else {
        return false;
    };
    backend.move_matched(index, dest).is_ok()
}

struct BackendWindows<'a, B> {
    backend: &'a B,
    screens: &'a [Bounds],
    titles: bool,
}

impl<B: AxBackend> LiveWindows for BackendWindows<'_, B> {
    fn pid_alive(&self, pid: i32) -> bool {
        self.backend.pid_alive(pid)
    }

    fn app_name(&self, pid: i32) -> Option<String> {
        self.backend.app_name(pid)
    }

    fn cg_bounds(&self, pid: i32, window: u32) -> Option<Bounds> {
        self.backend.cg_bounds(pid, window)
    }

    fn ax_windows(&self, pid: i32) -> Vec<AxWindow> {
        self.backend.windows(pid, self.titles)
    }

    fn visible_screens(&self) -> Vec<Bounds> {
        self.screens.to_vec()
    }
}
