//! Fixed-step clock: 40 ms steps, at most two ticks, then a resync.

use shimeji_app::{Command, Runtime};
use shimeji_core::EngineSettings;
use shimeji_core::env::{Point, ScreenSample, Snapshot};
use shimeji_core::model::{
    Action, ActionKind, Animation, Behavior, Builtin, Character, Information, OrderedMap, Param,
    Pose, Schema, ScriptSource,
};

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

fn sitter() -> Character {
    let mut params = OrderedMap::new();
    params.insert("Name", param("Name", "Sit"));
    let action = Action {
        name: Some("Sit".to_string()),
        schema: Schema::English,
        kind: ActionKind::Stay,
        degraded_from: None,
        params,
        animations: vec![Animation {
            condition: None,
            is_turn: false,
            poses: vec![Pose {
                image_source: Some("/sit.png".to_string()),
                image: None,
                image_right_source: None,
                image_right: None,
                right_sibling: None,
                anchor_x: 0,
                anchor_y: 0,
                velocity_x: 0,
                velocity_y: 0,
                duration_ticks: 8,
                sound: None,
                volume: None,
                line: 1,
            }],
            hotspots: Vec::new(),
            line: 1,
        }],
        children: Vec::new(),
        line: 1,
    };
    let mut actions = OrderedMap::new();
    actions.insert("Sit", action);
    let mut behaviors = OrderedMap::new();
    behaviors.insert(
        "Sit",
        Behavior {
            name: "Sit".to_string(),
            action: "Sit".to_string(),
            frequency: 1,
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

/// Fall is selectable only once a screen exists and the pet is above it.
/// An empty world matches nothing, and that selection teleports.
fn faller() -> Character {
    let action = Action {
        name: Some("Fall".to_string()),
        schema: Schema::English,
        kind: ActionKind::Builtin(Builtin::Fall),
        degraded_from: None,
        params: OrderedMap::new(),
        animations: vec![Animation {
            condition: None,
            is_turn: false,
            poses: vec![Pose {
                image_source: Some("/fall.png".to_string()),
                image: None,
                image_right_source: None,
                image_right: None,
                right_sibling: None,
                anchor_x: 0,
                anchor_y: 0,
                velocity_x: 0,
                velocity_y: 0,
                duration_ticks: 8,
                sound: None,
                volume: None,
                line: 1,
            }],
            hotspots: Vec::new(),
            line: 1,
        }],
        children: Vec::new(),
        line: 1,
    };
    let mut actions = OrderedMap::new();
    actions.insert("Fall", action);
    let (condition, error) = ScriptSource::parse_attribute(
        "#{mascot.environment.screen.height > 0 && mascot.anchor.y < mascot.environment.screen.top}",
    );
    assert!(error.is_none(), "{error:?}");
    let mut behaviors = OrderedMap::new();
    behaviors.insert(
        "Fall",
        Behavior {
            name: "Fall".to_string(),
            action: "Fall".to_string(),
            frequency: 100,
            hidden: false,
            toggleable: false,
            conditions: vec![condition],
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

fn desk() -> Snapshot {
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
        active_window: None,
        multiscreen: true,
    }
}

fn wide() -> Snapshot {
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

#[test]
fn timestamps_40ms_apart_tick_once_each() {
    let mut runtime = Runtime::new(&sitter(), 1);
    let world = wide();
    assert_eq!(runtime.on_frame(0.0, &world).ticks, 0);
    assert_eq!(runtime.on_frame(40.0, &world).ticks, 1);
    assert_eq!(runtime.on_frame(80.0, &world).ticks, 1);
    assert_eq!(runtime.on_frame(120.0, &world).ticks, 1);
}

#[test]
fn a_200ms_gap_ticks_twice_then_resyncs() {
    let mut runtime = Runtime::new(&sitter(), 1);
    let world = wide();
    assert_eq!(runtime.on_frame(0.0, &world).ticks, 0);
    let burst = runtime.on_frame(200.0, &world);
    assert_eq!(burst.ticks, 2);
    assert!(burst.resynced);
    let next = runtime.on_frame(240.0, &world);
    assert_eq!(next.ticks, 1);
    assert!(!next.resynced);
}

#[test]
fn timestamps_16_7ms_apart_average_25_ticks_per_second() {
    let mut runtime = Runtime::new(&sitter(), 1);
    let world = wide();
    let frames = 600;
    let mut ticks = 0;
    for index in 0..=frames {
        ticks += runtime.on_frame(index as f64 * 16.7, &world).ticks;
    }
    let span_ms = frames as f64 * 16.7;
    let rate = ticks as f64 / (span_ms / 1000.0);
    assert!(
        (rate - 25.0).abs() < 0.5,
        "rate {rate} from {ticks} ticks over {span_ms} ms"
    );
}

#[test]
fn pause_drops_the_backlog_and_move_frozen_still_applies() {
    let mut runtime = Runtime::new(&sitter(), 1);
    let world = wide();
    runtime.push(Command::Spawn);
    assert_eq!(runtime.on_frame(0.0, &world).ticks, 0);
    assert_eq!(runtime.population(), 0, "spawn waits for a tick boundary");
    let started = runtime.on_frame(40.0, &world);
    assert_eq!(started.ticks, 1);
    let id = started.spawned[0];
    assert_eq!(runtime.population(), 1);

    runtime.push(Command::Pause(true));
    let frozen = runtime.on_frame(240.0, &world);
    assert_eq!(frozen.ticks, 0);
    assert!(runtime.paused());

    runtime.push(Command::MoveFrozen {
        id,
        anchor: Point::new(12, 34),
    });
    let moved = runtime.on_frame(280.0, &world);
    assert_eq!(moved.ticks, 0);
    assert_eq!(moved.frozen.len(), 1);
    assert!(moved.frozen[0].anchor_changed);
    assert_eq!(runtime.frames()[0].anchor, Point::new(12, 34));

    runtime.push(Command::Pause(false));
    let resumed = runtime.on_frame(320.0, &world);
    assert_eq!(resumed.ticks, 1);
    assert!(!resumed.resynced);
}

#[test]
fn set_paused_freezes_immediately_and_set_behavior_switches() {
    let mut character = sitter();
    character.behaviors.insert(
        "Walk",
        shimeji_core::model::Behavior {
            name: "Walk".to_string(),
            action: "Sit".to_string(),
            frequency: 0,
            hidden: false,
            toggleable: true,
            conditions: Vec::new(),
            params: OrderedMap::new(),
            next_lists: Vec::new(),
            next_additive: true,
            line: 1,
        },
    );
    let mut runtime = Runtime::new(&character, 1);
    let world = wide();
    runtime.push(Command::Spawn);
    runtime.on_frame(0.0, &world);
    runtime.on_frame(40.0, &world);
    assert_eq!(runtime.population(), 1);
    assert_eq!(runtime.frames()[0].behavior, "Sit");

    runtime.set_paused(true);
    let frozen = runtime.on_frame(200.0, &world);
    assert_eq!(frozen.ticks, 0);
    assert!(runtime.paused());

    runtime.set_paused(false);
    let id = runtime.frames()[0].id;
    runtime.push(Command::SetBehavior {
        id: Some(id),
        name: "Walk".to_string(),
    });
    runtime.push(Command::SetBehaviorEnabled {
        name: "Walk".to_string(),
        enabled: false,
    });
    let switched = runtime.on_frame(240.0, &world);
    assert_eq!(switched.ticks, 1);
    assert_eq!(runtime.frames()[0].behavior, "Walk");
}

#[test]
fn interject_waits_for_a_tick_boundary() {
    let mut character = sitter();
    character.behaviors.insert(
        "Wave",
        shimeji_core::model::Behavior {
            name: "Wave".to_string(),
            action: "Sit".to_string(),
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
    let mut runtime = Runtime::new(&character, 1);
    let world = wide();
    runtime.on_frame(0.0, &world);
    runtime.push(Command::Spawn);
    runtime.on_frame(40.0, &world);
    assert_eq!(runtime.frames()[0].behavior, "Sit");
    let id = runtime.frames()[0].id;
    runtime.push(Command::InterjectBehavior {
        mascot: id,
        behavior: "Wave".to_string(),
    });
    let early = runtime.on_frame(50.0, &world);
    assert_eq!(early.ticks, 0);
    assert_eq!(runtime.frames()[0].behavior, "Sit");
    runtime.on_frame(90.0, &world);
    assert_eq!(runtime.frames()[0].behavior, "Wave");
}

#[test]
fn deferred_commands_run_once_before_the_burst() {
    let mut runtime = Runtime::new(&sitter(), 1);
    let world = wide();
    runtime.push(Command::Spawn);
    runtime.push(Command::Spawn);
    runtime.push(Command::Settings(EngineSettings {
        scaling: 2.0,
        breeding: false,
        transients: true,
        throwing: true,
        keep_on_screen: false,
        desktop_border_priority: true,
    }));
    runtime.on_frame(0.0, &world);
    runtime.on_frame(20.0, &world);
    assert_eq!(runtime.population(), 0);
    assert_eq!(runtime.settings().scaling, 1.0);
    let frame = runtime.on_frame(60.0, &world);
    assert_eq!(frame.ticks, 1);
    assert_eq!(runtime.population(), 2);
    assert_eq!(runtime.settings().scaling, 2.0);
    assert!(!runtime.settings().breeding);
    assert!(runtime.settings().desktop_border_priority);
}

#[test]
fn spawn_at_above_the_screen_keeps_its_x() {
    let mut runtime = Runtime::new(&faller(), 1);
    let world = desk();
    let above = -256;

    runtime.push(Command::SpawnAt(Point::new(400, above)));
    let opening = runtime.on_frame(0.0, &world);
    assert_eq!(opening.ticks, 0);
    assert_eq!(opening.spawned.len(), 1);

    let fell = runtime.on_frame(40.0, &world);
    assert_eq!(fell.ticks, 1);
    let first = &runtime.frames()[0];
    assert_eq!(first.behavior, "Fall");
    assert_eq!(first.anchor.x, 400);
    assert!(
        first.anchor.y > above,
        "fell from {above}, y is {}",
        first.anchor.y
    );

    runtime.push(Command::SpawnAt(Point::new(120, above)));
    let later = runtime.on_frame(80.0, &world);
    assert_eq!(later.ticks, 1);
    assert_eq!(later.spawned.len(), 1);
    let born_id = later.spawned[0];
    let born = runtime
        .frames()
        .into_iter()
        .find(|frame| frame.id == born_id)
        .expect("spawned on this frame");
    assert_eq!(born.behavior, "Fall");
    assert_eq!(born.anchor.x, 120);
    assert!(born.anchor.y > above, "y is {}", born.anchor.y);
}

#[test]
fn spawn_at_appears_on_the_first_frame_while_paused() {
    let mut runtime = Runtime::new(&sitter(), 1);
    let world = wide();
    runtime.set_paused(true);
    runtime.push(Command::SpawnAt(Point::new(10, 20)));
    let spawned = runtime.on_frame(0.0, &world).spawned.to_vec();
    assert_eq!(spawned.len(), 1);
    assert_eq!(runtime.population(), 1);
    assert_eq!(runtime.on_frame(400.0, &world).ticks, 0);
}

#[test]
fn set_mascot_cap_applies_before_a_paused_clock_would_tick() {
    let mut runtime = Runtime::new(&sitter(), 1);
    let world = wide();
    runtime.push(Command::SetMascotCap(1));
    runtime.push(Command::Spawn);
    runtime.push(Command::Spawn);
    runtime.on_frame(0.0, &world);
    assert_eq!(runtime.population(), 0);
    runtime.on_frame(40.0, &world);
    assert_eq!(runtime.population(), 1);
}
