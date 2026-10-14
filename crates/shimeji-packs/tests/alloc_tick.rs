//! Heap traffic inside `engine.tick` after warmup.
//!
//! This file is its own integration-test binary so the counting allocator does
//! not wrap the rest of the suite.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use shimeji_core::Engine;
use shimeji_core::env::{Point, ScreenSample, Snapshot};
use shimeji_core::model::{
    Action, ActionKind, Animation, Behavior, Character, Information, OrderedMap, Param, Pose,
    Schema, ScriptSource,
};
use shimeji_packs::load_dir;

struct Counting;

static ARMED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static TRACE: Mutex<Option<String>> = Mutex::new(None);
/// The counter is process-global. Two tests in this binary must not overlap.
static GATE: Mutex<()> = Mutex::new(());

thread_local! {
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

fn note() {
    if !ARMED.load(Ordering::Relaxed) {
        return;
    }
    BUSY.with(|busy| {
        if busy.get() {
            return;
        }
        busy.set(true);
        let seen = ALLOCS.fetch_add(1, Ordering::Relaxed);
        if seen == 0 {
            let text = std::backtrace::Backtrace::force_capture().to_string();
            if let Ok(mut slot) = TRACE.lock() {
                *slot = Some(text);
            }
        }
        busy.set(false);
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn world() -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".to_string(),
            x: -4500,
            y: -4500,
            width: 9000,
            height: 9000,
            work: None,
        }],
        cursor: Point::new(0, 0),
        active_window: None,
        multiscreen: true,
    }
}

fn param(key: &str, raw: &str) -> Param {
    let (value, error) = ScriptSource::parse_attribute(raw);
    assert!(error.is_none(), "{raw}");
    Param {
        key: key.to_string(),
        source_name: key.to_string(),
        value,
        line: 1,
    }
}

fn pose(image: &str, velocity_x: i32) -> Pose {
    Pose {
        image_source: Some(image.to_string()),
        image: None,
        image_right_source: None,
        image_right: None,
        right_sibling: None,
        anchor_x: 0,
        anchor_y: 0,
        velocity_x,
        velocity_y: 0,
        duration_ticks: 1,
        sound: None,
        volume: None,
        line: 1,
    }
}

fn character(kind: ActionKind, pairs: &[(&str, &str)], poses: Vec<Pose>) -> Character {
    let mut params = OrderedMap::new();
    params.insert("Name", param("Name", "Act"));
    for (key, raw) in pairs {
        params.insert(*key, param(key, raw));
    }
    let action = Action {
        name: Some("Act".to_string()),
        schema: Schema::English,
        kind,
        degraded_from: None,
        params,
        animations: vec![Animation {
            condition: None,
            is_turn: false,
            poses,
            hotspots: Vec::new(),
            line: 1,
        }],
        children: Vec::new(),
        line: 1,
    };
    let mut actions = OrderedMap::new();
    actions.insert("Act", action);
    let mut behaviors = OrderedMap::new();
    behaviors.insert(
        "Act",
        Behavior {
            name: "Act".to_string(),
            action: "Act".to_string(),
            frequency: 0,
            hidden: false,
            toggleable: false,
            conditions: Vec::new(),
            params: OrderedMap::new(),
            next_lists: Vec::new(),
            next_additive: true,
            line: 1,
        },
    );
    Character {
        name: "test".to_string(),
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

fn measure(engine: &mut Engine, snapshot: &Snapshot) -> usize {
    ALLOCS.store(0, Ordering::SeqCst);
    if let Ok(mut slot) = TRACE.lock() {
        *slot = None;
    }
    ARMED.store(true, Ordering::SeqCst);
    engine.tick(snapshot);
    ARMED.store(false, Ordering::SeqCst);
    ALLOCS.load(Ordering::SeqCst)
}

fn trace() -> String {
    TRACE
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_default()
}

fn assert_steady(label: &str, engine: &mut Engine, snapshot: &Snapshot) {
    let count = measure(engine, snapshot);
    assert_eq!(
        count,
        0,
        "{label} allocated {count} times on a steady tick\n{}",
        trace()
    );
}

#[test]
fn sitting_and_walking_synthetic_ticks_allocate_nothing() {
    let _gate = GATE.lock().expect("alloc gate");
    let snapshot = world();
    let sit = character(ActionKind::Stay, &[], vec![pose("/sit.png", 0)]);
    let mut engine = Engine::new(&sit, 1);
    engine.apply(&snapshot);
    let id = engine.spawn_behavior("Act").unwrap();
    engine.move_frozen(id, Point::new(10, 10));
    for _ in 0..30 {
        engine.tick(&snapshot);
    }
    assert_steady("sit", &mut engine, &snapshot);

    let walk = character(
        ActionKind::Move,
        &[("TargetX", "100000")],
        vec![pose("/a.png", -2), pose("/b.png", -2)],
    );
    let mut engine = Engine::new(&walk, 1);
    engine.apply(&snapshot);
    let id = engine.spawn_behavior("Act").unwrap();
    engine.move_frozen(id, Point::new(10, 10));
    for _ in 0..30 {
        engine.tick(&snapshot);
    }
    assert_steady("walk", &mut engine, &snapshot);
}

#[test]
fn shime_steady_tick_reports_its_allocations() {
    let _gate = GATE.lock().expect("alloc gate");
    let loaded = load_dir(root().join("assets/packs/shime")).expect("shime");
    let character = loaded.pack.character("shime").expect("shime").clone();
    let snapshot = world();
    let mut engine = Engine::new(&character, 1);
    engine.apply(&snapshot);
    engine.spawn();
    for _ in 0..2_000 {
        engine.tick(&snapshot);
    }
    let count = measure(&mut engine, &snapshot);
    eprintln!("shime steady tick allocations: {count}");
    if count != 0 {
        eprintln!("{}", trace());
    }
    assert_eq!(
        count,
        0,
        "shime steady tick allocated {count} times\n{}",
        trace()
    );
}
