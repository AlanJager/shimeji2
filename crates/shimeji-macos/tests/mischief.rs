//! Mischief queue, carry gate, and the non-blocking window worker.
//!
//! No test here talks to the window server. The slow backend sleeps on the
//! worker thread; `request_move` only enqueues.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use shimeji_app::journal::{Bounds, CarryRefusal, EndSpec, LaunchId, Operation, StartSpec};
use shimeji_app::mischief_state::Phase;
use shimeji_app::{Command, Runtime};
use shimeji_core::JavaRandom;
use shimeji_core::env::{Point, ScreenSample, Snapshot, WindowSample};
use shimeji_core::model::{
    Action, ActionKind, Animation, Behavior, Builtin, Character, Information, OrderedMap, Param,
    Pose, Schema, ScriptSource,
};
use shimeji_core::window_control::{WindowControl, WindowEvent, WindowMove};
use shimeji_macos::mischief::{
    ACCESSIBILITY_PANE, AxBackend, CarryTracker, Decision, JournalPort, MoveMiss, MoveQueue,
    PendingMove, Service, TRUST_INTERVAL, TargetView, WindowHandle, enabled_from_env, menu_item,
    phase_words, poll_due, trust_on_activate,
};

fn bounds(x: i32, y: i32, width: i32, height: i32) -> Bounds {
    Bounds {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn coalesced_moves_keep_the_first_origin_and_pid() {
    let mut queue = MoveQueue::default();
    queue.push(PendingMove {
        id: 7,
        pid: 4,
        window: 7,
        origin: bounds(1, 2, 30, 40),
        dest: bounds(5, 6, 30, 40),
    });
    queue.push(PendingMove {
        id: 7,
        pid: 9,
        window: 8,
        origin: bounds(0, 0, 1, 1),
        dest: bounds(9, 9, 30, 40),
    });
    let pending = queue.take(7).expect("queued");
    assert_eq!(pending.pid, 4);
    assert_eq!(pending.window, 7);
    assert_eq!(pending.origin, bounds(1, 2, 30, 40));
    assert_eq!(pending.dest, bounds(9, 9, 30, 40));
    assert!(queue.take(7).is_none());
}

#[test]
fn the_first_move_holds_until_the_gate_and_release_ends_the_carry() {
    let origin = bounds(10, 20, 30, 40);
    let mut tracker = CarryTracker::new();
    let Decision::Hold(spec) = tracker.consider(7, 4, 7, origin, 1) else {
        panic!("the first request holds for the journal");
    };
    assert_eq!(spec.op, Operation::Carry);
    assert_eq!(spec.bounds, origin);
    assert_eq!(spec.pid, 4);
    assert_eq!(spec.window, 7);
    assert!(tracker.holding(7));
    assert_eq!(
        tracker.consider(7, 99, 7, bounds(0, 0, 1, 1), 2),
        Decision::Waiting
    );
    assert!(
        tracker.finish_inactive(&[7], 3).is_empty(),
        "a journal gate is not a quiet end"
    );
    assert_eq!(tracker.allow(7, 4), None);
    assert!(matches!(
        tracker.consider(7, 4, 7, origin, 5),
        Decision::Apply { slot: 0 }
    ));

    let ended = tracker.finish_inactive(&[7], 6);
    assert!(
        ended.is_empty(),
        "an id still held by a carry action stays open"
    );
    let ended = tracker.finish_inactive(&[], 7);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].origin, origin);
    assert_eq!(ended[0].end.window, 7);
    assert_eq!(ended[0].end.op, Operation::Carry);

    let Decision::Hold(_) = tracker.consider(8, 4, 8, origin, 8) else {
        panic!("a new window holds again");
    };
    assert!(tracker.finish(8, 9).is_none(), "cancel leaves the hold");
    let end = tracker
        .allow(8, 10)
        .expect("a cancelled hold ends when the gate answers");
    assert_eq!(
        end,
        EndSpec {
            op: Operation::Carry,
            pid: 4,
            window: 8,
            slot: 1,
            at_ms: 10,
        }
    );
}

#[test]
fn a_refused_gate_drops_the_hold() {
    let mut tracker = CarryTracker::new();
    let Decision::Hold(_) = tracker.consider(9, 1, 9, bounds(1, 1, 2, 2), 0) else {
        panic!("hold");
    };
    tracker.refuse(9);
    assert!(!tracker.holding(9));
    assert!(matches!(
        tracker.consider(9, 1, 9, bounds(1, 1, 2, 2), 1),
        Decision::Hold(_)
    ));
}

#[test]
fn permission_polls_while_the_clock_runs_and_once_on_activate() {
    assert!(!enabled_from_env(None));
    assert!(!enabled_from_env(Some("")));
    assert!(!enabled_from_env(Some("true")));
    assert!(enabled_from_env(Some(" 1 ")));
    assert!(poll_due(Phase::On, true, TRUST_INTERVAL));
    assert!(poll_due(Phase::AwaitingPermission, true, TRUST_INTERVAL));
    assert!(!poll_due(Phase::On, false, TRUST_INTERVAL));
    assert!(!poll_due(
        Phase::On,
        true,
        TRUST_INTERVAL - Duration::from_millis(1)
    ));
    assert!(!poll_due(Phase::PermissionMissing, true, TRUST_INTERVAL));
    assert!(!poll_due(Phase::Off, true, TRUST_INTERVAL));
    assert!(trust_on_activate(Phase::PermissionMissing));
    assert!(trust_on_activate(Phase::AwaitingPermission));
    assert!(trust_on_activate(Phase::On));
    assert!(!trust_on_activate(Phase::Off));
    assert!(ACCESSIBILITY_PANE.contains("Privacy_Accessibility"));
}

#[test]
fn an_off_handle_produces_no_window_event() {
    let mut handle = WindowHandle::default();
    handle.request_move(&WindowMove {
        id: 7,
        x: 1,
        y: 2,
        width: 3,
        height: 4,
    });
    assert!(handle.drain_events().is_empty());
}

#[test]
fn menu_title_and_checkmark_follow_the_phase() {
    let off = menu_item(Phase::Off);
    assert_eq!(off.title, "Mischief");
    assert!(!off.checked);
    assert_eq!(phase_words(Phase::Off), "Off");

    let on = menu_item(Phase::On);
    assert_eq!(on.title, "Mischief");
    assert!(on.checked);
    assert_eq!(phase_words(Phase::On), "On");

    let waiting = menu_item(Phase::AwaitingPermission);
    assert_eq!(waiting.title, "Mischief (waiting for Accessibility)");
    assert!(waiting.checked);
    assert_eq!(
        phase_words(Phase::AwaitingPermission),
        "Waiting for Accessibility"
    );

    let missing = menu_item(Phase::PermissionMissing);
    assert_eq!(missing.title, "Mischief (permission missing)");
    assert!(missing.checked);
    assert_eq!(phase_words(Phase::PermissionMissing), "Permission missing");
}

#[test]
fn a_handle_that_is_not_engaged_produces_no_window_event() {
    let (mut handle, _rx) = WindowHandle::pair();
    handle.request_move(&WindowMove {
        id: 7,
        x: 1,
        y: 2,
        width: 3,
        height: 4,
    });
    assert!(handle.drain_events().is_empty());
}

#[test]
fn an_engaged_handle_that_is_not_live_fails_the_move() {
    let (mut handle, _rx) = WindowHandle::pair();
    handle.set_engaged(true);
    handle.request_move(&WindowMove {
        id: 7,
        x: 1,
        y: 2,
        width: 3,
        height: 4,
    });
    assert_eq!(handle.drain_events(), vec![WindowEvent::Failed { id: 7 }]);
}

/// Off from boot to shutdown: no worker and no journal file. The trust probe
/// is not called, so this does not need a window server.
#[cfg(target_os = "macos")]
#[test]
fn mischief_off_from_boot_creates_no_journal_and_no_worker() {
    use shimeji_app::journal::{journal_path, slots_path};
    use shimeji_app::window_rules::WindowRules;
    use shimeji_macos::mischief::Session;

    let dir = std::env::temp_dir().join(format!(
        "shimeji-u11c-off-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut session = Session::boot(dir.clone(), false, WindowRules::default(), Vec::new());
    assert_eq!(session.phase(), Phase::Off);
    assert!(!session.worker_running());
    session.on_clock(true);
    session.on_activate();
    let mut handle = session.handle();
    handle.request_move(&WindowMove {
        id: 7,
        x: 1,
        y: 2,
        width: 3,
        height: 4,
    });
    assert!(session.drain().is_empty());
    session.set_enabled(false);
    assert!(!session.worker_running());
    session.shutdown();
    assert!(!journal_path(&dir).exists());
    assert!(!slots_path(&dir).exists());
    assert_eq!(session.phase(), Phase::Off);
    let _ = std::fs::remove_dir_all(&dir);
}

struct CountingBackend {
    stop: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
}

impl AxBackend for CountingBackend {
    fn windows(&self, _pid: i32, _with_title: bool) -> Vec<shimeji_app::journal::AxWindow> {
        Vec::new()
    }

    fn app_name(&self, _pid: i32) -> Option<String> {
        Some("TextEdit".to_string())
    }

    fn pid_alive(&self, pid: i32) -> bool {
        pid > 0
    }

    fn cg_bounds(&self, _pid: i32, _window: u32) -> Option<Bounds> {
        None
    }

    fn move_matched(&self, _index: usize, bounds: Bounds) -> Result<Bounds, MoveMiss> {
        self.started.store(true, Ordering::Release);
        let _ = self.stop.load(Ordering::Acquire);
        Ok(bounds)
    }
}

struct RefuseJournal;

impl JournalPort for RefuseJournal {
    fn request_begin(&mut self, _id: u64, _spec: StartSpec) {}

    fn poll_begin(&mut self, _id: u64) -> Option<Result<(), CarryRefusal>> {
        Some(Err(CarryRefusal::SyncTimeout))
    }

    fn write_end(&mut self, _spec: &EndSpec) {}

    fn write_applied(&mut self, _slot: u16, _pid: i32, _window: u32, _bounds: Bounds) {}

    fn write_header(&mut self, _launch: LaunchId) {}

    fn release_files(&mut self) {}

    fn flush(&mut self) {}
}

#[test]
fn a_refused_gate_fails_the_window_without_moving_it() {
    let (handle, rx) = WindowHandle::pair();
    handle.set_live(true);
    handle.publish(target_view());
    let started = Arc::new(AtomicBool::new(false));
    let started_flag = Arc::clone(&started);
    let service = Service::start_with(
        rx,
        &handle,
        move |stop| CountingBackend {
            stop,
            started: started_flag,
        },
        RefuseJournal,
        std::env::temp_dir(),
    );
    let mut windows = handle.clone();
    windows.request_move(&WindowMove {
        id: WINDOW_ID,
        x: 90,
        y: 100,
        width: 200,
        height: 150,
    });
    let mut failed = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        for event in handle.drain_events() {
            if matches!(event, WindowEvent::Failed { id: WINDOW_ID }) {
                failed = true;
            }
        }
        if failed {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(service);
    assert!(failed, "a refused start is a failed move");
    assert!(
        !started.load(Ordering::Acquire),
        "the worker does not move before the gate accepts"
    );
}

const WINDOW_ID: u64 = 7;
const WINDOW_BOUNDS: Bounds = Bounds {
    x: 100,
    y: 100,
    width: 200,
    height: 150,
};

fn target_view() -> TargetView {
    TargetView {
        pid: 42,
        window: WINDOW_ID,
        bounds: WINDOW_BOUNDS,
        screens: vec![bounds(0, 0, 800, 600)],
        rules: shimeji_app::window_rules::WindowRules::default(),
        titles: false,
    }
}

struct SlowBackend {
    stop: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

impl AxBackend for SlowBackend {
    fn windows(&self, _pid: i32, _with_title: bool) -> Vec<shimeji_app::journal::AxWindow> {
        vec![shimeji_app::journal::AxWindow {
            bounds: WINDOW_BOUNDS,
            subrole: shimeji_app::journal::AX_STANDARD_WINDOW.to_string(),
            position_settable: true,
            title: None,
        }]
    }

    fn app_name(&self, _pid: i32) -> Option<String> {
        Some("TextEdit".to_string())
    }

    fn pid_alive(&self, pid: i32) -> bool {
        pid > 0
    }

    fn cg_bounds(&self, _pid: i32, _window: u32) -> Option<Bounds> {
        Some(WINDOW_BOUNDS)
    }

    fn move_matched(&self, _index: usize, bounds: Bounds) -> Result<Bounds, MoveMiss> {
        self.started.store(true, Ordering::Release);
        let mut slept = Duration::ZERO;
        while slept < Duration::from_secs(5) {
            if self.stop.load(Ordering::Acquire) {
                return Ok(bounds);
            }
            thread::sleep(Duration::from_millis(50));
            slept += Duration::from_millis(50);
        }
        self.finished.store(true, Ordering::Release);
        Ok(bounds)
    }
}

struct InstantJournal {
    ready: HashSet<u64>,
}

impl JournalPort for InstantJournal {
    fn request_begin(&mut self, id: u64, _spec: StartSpec) {
        self.ready.insert(id);
    }

    fn poll_begin(&mut self, id: u64) -> Option<Result<(), CarryRefusal>> {
        if self.ready.remove(&id) {
            Some(Ok(()))
        } else {
            None
        }
    }

    fn write_end(&mut self, _spec: &EndSpec) {}

    fn write_applied(&mut self, _slot: u16, _pid: i32, _window: u32, _bounds: Bounds) {}

    fn write_header(&mut self, _launch: LaunchId) {}

    fn release_files(&mut self) {}

    fn flush(&mut self) {}
}

#[test]
fn a_five_second_move_does_not_stall_frames_and_the_pet_falls() {
    let (handle, rx) = WindowHandle::pair();
    handle.set_live(true);
    handle.publish(target_view());
    let started = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let started_flag = Arc::clone(&started);
    let finished_flag = Arc::clone(&finished);
    let service = Service::start_with(
        rx,
        &handle,
        move |stop| SlowBackend {
            stop,
            started: started_flag,
            finished: finished_flag,
        },
        InstantJournal {
            ready: HashSet::new(),
        },
        std::env::temp_dir(),
    );

    let mut rng = JavaRandom::new(1);
    let look_right = rng.next_double() < 0.5;
    let anchor = if look_right {
        Point::new(WINDOW_BOUNDS.x, WINDOW_BOUNDS.y + WINDOW_BOUNDS.height)
    } else {
        Point::new(
            WINDOW_BOUNDS.x + WINDOW_BOUNDS.width,
            WINDOW_BOUNDS.y + WINDOW_BOUNDS.height,
        )
    };
    let world = ie_world();
    let mut runtime = Runtime::with_windows(&ie_character(), 1, handle.clone());
    let mut now = 0.0;
    let mut slowest = Duration::ZERO;
    let mut saw_carry = false;
    let mut fell = false;
    let mut saw_timeout = false;

    let elapsed = frame(
        &mut runtime,
        &handle,
        now,
        &world,
        &mut saw_timeout,
        &started,
        &finished,
    );
    slowest = slowest.max(elapsed);
    now = 40.0;
    let elapsed = frame(
        &mut runtime,
        &handle,
        now,
        &world,
        &mut saw_timeout,
        &started,
        &finished,
    );
    slowest = slowest.max(elapsed);
    runtime.push(Command::SpawnAt(anchor));

    while now < 3_000.0 {
        now += 40.0;
        let elapsed = frame(
            &mut runtime,
            &handle,
            now,
            &world,
            &mut saw_timeout,
            &started,
            &finished,
        );
        slowest = slowest.max(elapsed);
        if runtime
            .frames()
            .first()
            .is_some_and(|item| item.behavior == "Carry")
        {
            saw_carry = true;
        }
        if runtime
            .frames()
            .first()
            .is_some_and(|item| item.behavior == "Fall")
        {
            fell = true;
        }
        thread::sleep(Duration::from_millis(15));
    }
    drop(service);

    assert!(
        slowest <= Duration::from_millis(4),
        "slowest frame {slowest:?}"
    );
    assert!(saw_carry, "the pet did not start the carry");
    assert!(fell, "the pet did not fall");
    assert!(
        saw_timeout,
        "no timeout failure while the move was still inside the backend"
    );
    assert!(
        !finished.load(Ordering::Acquire),
        "the 5 s move was still running when the test stopped it"
    );
}

fn frame(
    runtime: &mut Runtime<WindowHandle>,
    handle: &WindowHandle,
    now: f64,
    world: &Snapshot,
    saw_timeout: &mut bool,
    started: &AtomicBool,
    finished: &AtomicBool,
) -> Duration {
    for event in handle.drain_events() {
        if matches!(event, WindowEvent::Failed { id: WINDOW_ID })
            && started.load(Ordering::Acquire)
            && !finished.load(Ordering::Acquire)
        {
            *saw_timeout = true;
        }
        runtime.push(Command::Window(event));
    }
    let start = Instant::now();
    runtime.on_frame(now, world);
    start.elapsed()
}

fn ie_world() -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".to_string(),
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            work: None,
        }],
        cursor: Point::new(0, 0),
        active_window: Some(WindowSample {
            visible: true,
            id: WINDOW_ID,
            x: WINDOW_BOUNDS.x,
            y: WINDOW_BOUNDS.y,
            width: WINDOW_BOUNDS.width,
            height: WINDOW_BOUNDS.height,
        }),
        multiscreen: true,
    }
}

fn ie_character() -> Character {
    let carry = action(
        "Carry",
        ActionKind::Builtin(Builtin::WalkWithIe),
        &[("TargetX", "-1000")],
        -2,
    );
    let fall = action("Fall", ActionKind::Builtin(Builtin::Fall), &[], 0);
    let mut actions = OrderedMap::new();
    actions.insert("Carry", carry);
    actions.insert("Fall", fall);
    let mut behaviors = OrderedMap::new();
    behaviors.insert("Carry", behavior("Carry", 1));
    behaviors.insert("Fall", behavior("Fall", 0));
    Character {
        name: "ie".to_string(),
        schema: Schema::English,
        behavior_schema: Schema::English,
        image_root: String::new(),
        actions_file: "actions.xml".to_string(),
        behaviors_file: "behaviors.xml".to_string(),
        info_file: None,
        constants: OrderedMap::new(),
        actions,
        behaviors,
        behavior_tree: Vec::new(),
        info: Information::default(),
    }
}

fn action(name: &str, kind: ActionKind, pairs: &[(&str, &str)], velocity_x: i32) -> Action {
    let mut params = OrderedMap::new();
    for (key, raw) in pairs {
        let (value, error) = ScriptSource::parse_attribute(raw);
        assert!(error.is_none(), "{raw}");
        params.insert(
            *key,
            Param {
                key: (*key).to_string(),
                source_name: (*key).to_string(),
                value,
                line: 1,
            },
        );
    }
    Action {
        name: Some(name.to_string()),
        schema: Schema::English,
        kind,
        degraded_from: None,
        params,
        animations: vec![Animation {
            condition: None,
            is_turn: false,
            poses: vec![pose(velocity_x)],
            hotspots: Vec::new(),
            line: 1,
        }],
        children: Vec::new(),
        line: 1,
    }
}

fn pose(velocity_x: i32) -> Pose {
    Pose {
        image_source: Some("/ie.png".to_string()),
        image: None,
        image_right_source: None,
        image_right: None,
        right_sibling: None,
        anchor_x: 0,
        anchor_y: 0,
        velocity_x,
        velocity_y: 0,
        duration_ticks: 10_000,
        sound: None,
        volume: None,
        line: 1,
    }
}

fn behavior(name: &str, frequency: i32) -> Behavior {
    Behavior {
        name: name.to_string(),
        action: name.to_string(),
        frequency,
        hidden: false,
        toggleable: false,
        conditions: Vec::new(),
        params: OrderedMap::new(),
        next_lists: Vec::new(),
        next_additive: true,
        line: 1,
    }
}

#[derive(Default)]
struct CarryRecords {
    starts: Vec<StartSpec>,
    ends: Vec<EndSpec>,
}

struct RecordingJournal {
    records: Arc<std::sync::Mutex<CarryRecords>>,
    ready: std::collections::HashMap<u64, Result<(), CarryRefusal>>,
    log: shimeji_app::journal::FileLog,
    slots: shimeji_app::journal::FileSlots,
}

impl JournalPort for RecordingJournal {
    fn request_begin(&mut self, id: u64, spec: StartSpec) {
        self.records.lock().unwrap().starts.push(spec);
        let result = shimeji_app::journal::begin_carry(
            &mut self.log,
            &shimeji_app::journal::InstantClock::new(),
            &spec,
        );
        self.ready.insert(id, result);
    }
    fn poll_begin(&mut self, id: u64) -> Option<Result<(), CarryRefusal>> {
        self.ready.remove(&id)
    }
    fn write_end(&mut self, spec: &EndSpec) {
        shimeji_app::journal::write_end(&mut self.log, spec).unwrap();
        self.records.lock().unwrap().ends.push(*spec);
    }
    fn write_applied(&mut self, slot: u16, pid: i32, window: u32, bounds: Bounds) {
        shimeji_app::journal::write_applied(&mut self.slots, slot, pid, window, bounds).unwrap();
    }
    fn write_header(&mut self, launch: LaunchId) {
        shimeji_app::journal::write_header(&mut self.log, launch).unwrap();
    }
    fn release_files(&mut self) {}
    fn flush(&mut self) {}
}

#[derive(Clone)]
struct MovingBackend {
    current: Arc<std::sync::Mutex<Bounds>>,
    fail_next: Arc<AtomicBool>,
}
impl AxBackend for MovingBackend {
    fn windows(&self, _pid: i32, _titles: bool) -> Vec<shimeji_app::journal::AxWindow> {
        vec![shimeji_app::journal::AxWindow {
            bounds: *self.current.lock().unwrap(),
            subrole: shimeji_app::journal::AX_STANDARD_WINDOW.into(),
            position_settable: true,
            title: None,
        }]
    }
    fn app_name(&self, _pid: i32) -> Option<String> {
        Some("TextEdit".into())
    }
    fn pid_alive(&self, _pid: i32) -> bool {
        true
    }
    fn cg_bounds(&self, _pid: i32, _window: u32) -> Option<Bounds> {
        Some(*self.current.lock().unwrap())
    }
    fn move_matched(&self, _index: usize, bounds: Bounds) -> Result<Bounds, MoveMiss> {
        if self.fail_next.swap(false, Ordering::AcqRel) {
            return Err(MoveMiss);
        }
        *self.current.lock().unwrap() = bounds;
        Ok(bounds)
    }
}
impl shimeji_app::journal::LiveWindows for MovingBackend {
    fn pid_alive(&self, pid: i32) -> bool {
        AxBackend::pid_alive(self, pid)
    }
    fn app_name(&self, pid: i32) -> Option<String> {
        AxBackend::app_name(self, pid)
    }
    fn cg_bounds(&self, pid: i32, window: u32) -> Option<Bounds> {
        AxBackend::cg_bounds(self, pid, window)
    }
    fn ax_windows(&self, pid: i32) -> Vec<shimeji_app::journal::AxWindow> {
        self.windows(pid, false)
    }
    fn visible_screens(&self) -> Vec<Bounds> {
        vec![bounds(0, 0, 800, 600)]
    }
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let start = Instant::now();
    while !predicate() {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "worker did not answer"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn carry_journal_scenario(action_name: &str, remove: bool, fail: bool) {
    use shimeji_app::journal::{FileLog, FileSlots, journal_path, restore_directory, slots_path};
    use shimeji_macos::platform::window_env::PollingActions;
    static NEXT_SCENARIO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "shimeji-carry-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_SCENARIO.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let records = Arc::new(std::sync::Mutex::new(CarryRecords::default()));
    let mut journal = RecordingJournal {
        records: Arc::clone(&records),
        ready: Default::default(),
        log: FileLog::create(&journal_path(&dir)).unwrap(),
        slots: FileSlots::create(&slots_path(&dir)).unwrap(),
    };
    journal.write_header(LaunchId::from_bytes([17; 16]));
    let backend = MovingBackend {
        current: Arc::new(std::sync::Mutex::new(WINDOW_BOUNDS)),
        fail_next: Arc::new(AtomicBool::new(false)),
    };
    let live = backend.clone();
    let (mut handle, rx) = WindowHandle::pair();
    handle.set_live(true);
    handle.publish(target_view());
    let service = Service::start_with(rx, &handle, move |_| backend, journal, dir.clone());
    let actions = PollingActions::new(&ie_character());
    let mut frame = shimeji_core::MascotFrame {
        id: shimeji_core::MascotId(1),
        behavior: "Carry".into(),
        action: action_name.into(),
        anchor: Point::new(300, 250),
        image: None,
        look_right: false,
    };
    let window = WindowSample {
        visible: true,
        id: WINDOW_ID,
        x: 100,
        y: 100,
        width: 200,
        height: 150,
    };
    handle.request_move(&WindowMove {
        id: WINDOW_ID,
        x: 90,
        y: 100,
        width: 200,
        height: 150,
    });
    handle.note_activity(&actions.carry_windows(std::slice::from_ref(&frame), Some(&window)));
    wait_until(|| {
        handle
            .drain_events()
            .iter()
            .any(|event| matches!(event, WindowEvent::Moved { .. }))
    });
    // This frame is a live, stationary carry: no request_move call at all.
    for _ in 0..3 {
        handle.note_activity(&actions.carry_windows(std::slice::from_ref(&frame), Some(&window)));
    }
    thread::sleep(Duration::from_millis(25));
    assert!(
        records.lock().unwrap().ends.is_empty(),
        "a still pose split the carry"
    );
    let mut target = target_view();
    target.bounds = *live.current.lock().unwrap();
    handle.publish(target);
    live.fail_next.store(fail, Ordering::Release);
    handle.request_move(&WindowMove {
        id: WINDOW_ID,
        x: 80,
        y: 100,
        width: 200,
        height: 150,
    });
    wait_until(|| {
        handle.drain_events().iter().any(|event| {
            matches!(
                event,
                WindowEvent::Moved { .. } | WindowEvent::Failed { .. }
            )
        })
    });
    frame.action = "Stay".into();
    let frames = if remove { Vec::new() } else { vec![frame] };
    handle.note_activity(&actions.carry_windows(&frames, Some(&window)));
    wait_until(|| records.lock().unwrap().ends.len() == 1);
    drop(service);
    let recorded = records.lock().unwrap();
    assert_eq!(recorded.starts.len(), 1);
    assert_eq!(recorded.ends.len(), 1);
    assert_eq!(recorded.starts[0].bounds, WINDOW_BOUNDS);
    drop(recorded);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let restored = restore_directory(
        &dir,
        &shimeji_app::window_rules::WindowRules::default(),
        &live,
        now,
    )
    .unwrap();
    assert_eq!(restored.moves.len(), 1);
    assert_eq!(
        restored.moves[0].to, WINDOW_BOUNDS,
        "restore lost the original before the quiet pose"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_still_carry_is_one_journal_start_and_end_and_restores_the_first_original() {
    carry_journal_scenario("WalkWithIE", false, false);
}
#[test]
fn a_throw_action_closes_when_released_after_a_still_pose() {
    carry_journal_scenario("ThrowIE", false, false);
}
#[test]
fn removing_the_carrier_closes_its_journal() {
    carry_journal_scenario("FallWithIE", true, false);
}
#[test]
fn a_failed_move_closes_the_carry_and_preserves_restore() {
    carry_journal_scenario("WalkWithIE", false, true);
}
#[test]
fn release_while_waiting_for_the_journal_gate_closes_after_the_start_is_durable() {
    let mut tracker = CarryTracker::new();
    assert!(matches!(
        tracker.consider(7, 42, 7, WINDOW_BOUNDS, 0),
        Decision::Hold(_)
    ));
    assert!(tracker.finish_inactive(&[], 1).is_empty());
    assert!(tracker.allow(7, 2).is_some());
    assert!(!tracker.holding(7));
    assert_eq!(tracker.current(7), None);
}
