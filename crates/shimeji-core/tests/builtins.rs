//! Built-in actions, characterized against the pinned vendored reference classes.
//! The floor probe is 80 px (`Fall.java`), not the plan's 60 px snap.

#[path = "support.rs"]
mod support;

use shimeji_core::action::ActionError;
use shimeji_core::env::{Point, ScreenSample, Snapshot, WindowSample};
use shimeji_core::model::{
    Action, ActionKind, ActionNode, Animation, Builtin, FastFollow, Information, OrderedMap, Param,
    Pose, Schema, ScriptSource,
};
use shimeji_core::{Engine, JavaRandom, Mascot, RunningAction, compile_character};
use support::{behavior, program};

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

fn params(pairs: &[(&str, &str)]) -> OrderedMap<Param> {
    let mut map = OrderedMap::new();
    for (key, raw) in pairs {
        map.insert(*key, param(key, raw));
    }
    map
}

fn pose(image: &str, velocity_x: i32, velocity_y: i32, duration_ticks: i32) -> Pose {
    Pose {
        image_source: Some(image.to_string()),
        image: None,
        image_right_source: None,
        image_right: None,
        right_sibling: None,
        anchor_x: 0,
        anchor_y: 0,
        velocity_x,
        velocity_y,
        duration_ticks,
        sound: None,
        volume: None,
        line: 1,
    }
}

fn animation(poses: Vec<Pose>) -> Animation {
    Animation {
        condition: None,
        is_turn: false,
        poses,
        hotspots: Vec::new(),
        line: 1,
    }
}

/// `Name` is the trace column. `Action.name` is only the map key.
fn act(
    name: &str,
    schema: Schema,
    kind: ActionKind,
    pairs: &[(&str, &str)],
    poses: Vec<Pose>,
    children: Vec<ActionNode>,
) -> Action {
    let mut map = params(pairs);
    if !map.contains("Name") {
        map.insert("Name", param("Name", name));
    }
    Action {
        name: Some(name.to_string()),
        schema,
        kind,
        degraded_from: None,
        params: map,
        animations: if poses.is_empty() {
            Vec::new()
        } else {
            vec![animation(poses)]
        },
        children,
        line: 1,
    }
}

fn only(name: &str, body: Action) -> RunningAction {
    let mut actions = OrderedMap::new();
    actions.insert(name, body);
    let character = shimeji_core::model::Character {
        name: "test".to_string(),
        schema: Schema::English,
        behavior_schema: Schema::English,
        image_root: String::new(),
        actions_file: "actions.xml".to_string(),
        behaviors_file: "behaviors.xml".to_string(),
        info_file: None,
        constants: OrderedMap::new(),
        actions,
        behaviors: OrderedMap::new(),
        behavior_tree: Vec::new(),
        info: Information::default(),
    };
    compile_character(&character).instantiate(name).unwrap()
}

fn air(cursor: Point, window: Option<WindowSample>) -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".to_string(),
            x: 0,
            y: -20_000,
            width: 4_000,
            height: 40_000,
            work: None,
        }],
        cursor,
        active_window: window,
        multiscreen: true,
    }
}

fn plant(mascot: &mut Mascot, snapshot: &Snapshot) {
    let mut world = shimeji_core::env::Environment::new();
    world.apply(snapshot);
    mascot.sync(&world, true);
}

fn step(
    action: &mut RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<(), ActionError> {
    if !action.has_next(mascot, rng)? {
        return Err(ActionError::NoPose);
    }
    action.next(mascot, rng)?;
    mascot.time += 1;
    Ok(())
}

fn draws_between(before: &JavaRandom, after: &JavaRandom) -> u64 {
    let mut cursor = before.clone();
    for count in 0..8 {
        let mut ahead = cursor.clone();
        let mut live = after.clone();
        let matched = (0..3).all(|_| ahead.next_double().to_bits() == live.next_double().to_bits());
        if matched {
            return count;
        }
        cursor.next_double();
    }
    u64::MAX
}

fn number(value: shimeji_core::Value) -> f64 {
    value.as_number().expect("number")
}

#[test]
fn fall_approaches_terminal_velocity_and_lands_on_a_floor_300_below() {
    let mut action = only(
        "Drop",
        act(
            "Drop",
            Schema::English,
            ActionKind::Builtin(Builtin::Fall),
            &[],
            vec![pose("/fall.png", 0, 0, 1)],
            Vec::new(),
        ),
    );
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(10, 0);
    let mut rng = JavaRandom::new(1);
    let open = air(Point::new(0, 0), None);
    plant(&mut mascot, &open);
    action.init(&mut mascot, &mut rng).unwrap();
    for _ in 0..400 {
        step(&mut action, &mut mascot, &mut rng).unwrap();
    }
    let velocity = number(action.read("VelocityY", &mascot, &mut rng).unwrap());
    assert!(
        (velocity - 20.0).abs() < 1e-6,
        "terminal velocity {velocity}, gravity/resistanceY = 20"
    );
    let before = mascot.anchor.y;
    step(&mut action, &mut mascot, &mut rng).unwrap();
    // `modY + (velocityY % 1)` is added before `%= 1`, so one step at the
    // limit rounds to 19, 20, 21, or 22. The mean over a few steps is 20.
    let step_px = mascot.anchor.y - before;
    assert!(
        (19..=22).contains(&step_px),
        "terminal step {step_px} px, velocity {velocity}"
    );

    let mut action = only(
        "Drop",
        act(
            "Drop",
            Schema::English,
            ActionKind::Builtin(Builtin::Fall),
            &[],
            vec![pose("/fall.png", 0, 0, 1)],
            Vec::new(),
        ),
    );
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(10, 100);
    let floor = air(
        Point::new(0, 0),
        Some(WindowSample {
            visible: true,
            id: 0,
            x: 0,
            y: 400,
            width: 400,
            height: 80,
        }),
    );
    plant(&mut mascot, &floor);
    action.init(&mut mascot, &mut rng).unwrap();
    let mut ticks = 0;
    while action.has_next(&mut mascot, &mut rng).unwrap() {
        action.next(&mut mascot, &mut rng).unwrap();
        mascot.time += 1;
        ticks += 1;
        assert!(ticks < 80, "never landed, y={}", mascot.anchor.y);
    }
    assert_eq!(mascot.anchor.y, 400, "landed after {ticks} ticks");
    assert!(ticks > 10, "fell {ticks} ticks to cover 300 px");
}

#[test]
fn floor_60_above_the_fall_path_is_caught_and_90_is_not() {
    // First tick from rest moves +2. A floor 60 px above that end is inside
    // the 80 px probe. A floor 90 px above it is not.
    let caught = fall_one_tick(42);
    assert_eq!(caught, 42);
    let missed = fall_one_tick(12);
    assert_eq!(missed, 102);
}

fn fall_one_tick(floor_y: i32) -> i32 {
    let mut action = only(
        "Drop",
        act(
            "Drop",
            Schema::English,
            ActionKind::Builtin(Builtin::Fall),
            &[],
            vec![pose("/fall.png", 0, 0, 1)],
            Vec::new(),
        ),
    );
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(10, 100);
    let mut rng = JavaRandom::new(1);
    plant(
        &mut mascot,
        &air(
            Point::new(0, 0),
            Some(WindowSample {
                visible: true,
                id: 0,
                x: 0,
                y: floor_y,
                width: 400,
                height: 50,
            }),
        ),
    );
    action.init(&mut mascot, &mut rng).unwrap();
    step(&mut action, &mut mascot, &mut rng).unwrap();
    mascot.anchor.y
}

#[test]
fn jump_follows_the_arc_and_ends_at_distance_zero() {
    let mut action = only(
        "Hop",
        act(
            "Hop",
            Schema::English,
            ActionKind::Builtin(Builtin::Jump),
            &[("TargetX", "100"), ("TargetY", "0")],
            vec![pose("/jump.png", 0, 0, 1)],
            Vec::new(),
        ),
    );
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();
    step(&mut action, &mut mascot, &mut rng).unwrap();
    assert_eq!(mascot.anchor, Point::new(18, -9));
    let mut ticks = 1;
    while action.has_next(&mut mascot, &mut rng).unwrap() {
        action.next(&mut mascot, &mut rng).unwrap();
        mascot.time += 1;
        ticks += 1;
        assert!(ticks < 40, "jump did not arrive");
    }
    assert_eq!(mascot.anchor, Point::new(100, 0));
    assert!(ticks > 1);
}

#[test]
fn dragged_draws_once_on_the_escape_tick_and_not_in_has_next() {
    let mut action = only(
        "Drag",
        act(
            "Drag",
            Schema::English,
            ActionKind::Builtin(Builtin::Dragged),
            &[],
            vec![pose("/drag.png", 0, 0, 8)],
            Vec::new(),
        ),
    );
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    plant(&mut mascot, &air(Point::new(0, 0), None));
    action.init(&mut mascot, &mut rng).unwrap();
    for time in 0..250 {
        let before_has = rng.clone();
        assert!(
            action.has_next(&mut mascot, &mut rng).unwrap(),
            "time {time}"
        );
        assert_eq!(
            draws_between(&before_has, &rng),
            0,
            "hasNext drew at {time}"
        );
        let before_tick = rng.clone();
        action.next(&mut mascot, &mut rng).unwrap();
        let draws = draws_between(&before_tick, &rng);
        if time == 249 {
            assert_eq!(draws, 1, "escape tick draws once");
        } else {
            assert_eq!(draws, 0, "tick {time} drew");
        }
        mascot.time += 1;
    }
}

#[test]
fn foot_x_spring_uses_the_schema_name() {
    assert_eq!(spring(Schema::English, "FootX"), 8.0);
    assert_eq!(spring(Schema::Japanese, "footX"), 8.0);
}

fn spring(schema: Schema, slot: &str) -> f64 {
    let mut action = only(
        "Drag",
        act(
            "Drag",
            schema,
            ActionKind::Builtin(Builtin::Dragged),
            &[],
            vec![pose("/drag.png", 0, 0, 8)],
            Vec::new(),
        ),
    );
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    plant(&mut mascot, &air(Point::new(0, 0), None));
    action.init(&mut mascot, &mut rng).unwrap();
    plant(&mut mascot, &air(Point::new(100, 0), None));
    step(&mut action, &mut mascot, &mut rng).unwrap();
    number(action.read(slot, &mascot, &mut rng).unwrap())
}

#[test]
fn regist_holds_inside_four_pixels_and_draws_once_when_it_ends() {
    let mut action = only(
        "Hold",
        act(
            "Hold",
            Schema::English,
            ActionKind::Builtin(Builtin::Regist),
            &[],
            vec![pose("/regist.png", 0, 0, 3)],
            Vec::new(),
        ),
    );
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(20, 40);
    let mut rng = JavaRandom::new(1);
    plant(&mut mascot, &air(Point::new(20, 40), None));
    action.init(&mut mascot, &mut rng).unwrap();
    assert!(action.has_next(&mut mascot, &mut rng).unwrap());
    plant(&mut mascot, &air(Point::new(24, 40), None));
    assert!(action.has_next(&mut mascot, &mut rng).unwrap());
    plant(&mut mascot, &air(Point::new(25, 40), None));
    assert!(!action.has_next(&mut mascot, &mut rng).unwrap());

    plant(&mut mascot, &air(Point::new(20, 40), None));
    assert!(step(&mut action, &mut mascot, &mut rng).is_ok());
    assert!(step(&mut action, &mut mascot, &mut rng).is_ok());
    let before = rng.clone();
    let error = step(&mut action, &mut mascot, &mut rng).unwrap_err();
    assert!(matches!(error, ActionError::LostGround));
    assert_eq!(draws_between(&before, &rng), 1);
}

#[test]
fn look_and_offset_apply_during_sequence_init() {
    let look = act(
        "Turn",
        Schema::English,
        ActionKind::Builtin(Builtin::Look),
        &[],
        Vec::new(),
        Vec::new(),
    );
    let stand = act(
        "Stand",
        Schema::English,
        ActionKind::Stay,
        &[],
        vec![pose("/stand.png", 0, 0, 4)],
        Vec::new(),
    );
    let sequence = act(
        "Seq",
        Schema::English,
        ActionKind::Sequence,
        &[],
        Vec::new(),
        vec![ActionNode::Action(look), ActionNode::Action(stand)],
    );
    let mut running = only("Seq", sequence);
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    running.init(&mut mascot, &mut rng).unwrap();
    assert!(mascot.look_right);
    assert!(running.has_next(&mut mascot, &mut rng).unwrap());
    assert_eq!(running.active_name(), "Stand");

    let shift = act(
        "Shift",
        Schema::English,
        ActionKind::Builtin(Builtin::Offset),
        &[("X", "4"), ("Y", "-2")],
        Vec::new(),
        Vec::new(),
    );
    let after = act(
        "After",
        Schema::English,
        ActionKind::Stay,
        &[],
        vec![pose("/stand.png", 0, 0, 4)],
        Vec::new(),
    );
    let sequence = act(
        "MoveSeq",
        Schema::English,
        ActionKind::Sequence,
        &[],
        Vec::new(),
        vec![ActionNode::Action(shift), ActionNode::Action(after)],
    );
    let mut running = only("MoveSeq", sequence);
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(10, 10);
    running.init(&mut mascot, &mut rng).unwrap();
    assert_eq!(mascot.anchor, Point::new(14, 8));
    assert_eq!(running.active_name(), "After");
}

#[test]
fn breed_at_the_global_cap_spawns_nothing_and_draws_nothing() {
    let (mut engine, id) = breed_engine("#{Math.random()}", 1);
    engine.tick(&on_screen());
    let before = engine.rng_draws();
    engine.tick(&on_screen());
    assert_eq!(engine.rng_draws(), before);
    assert_eq!(engine.population(), 1);
    assert_eq!(engine.frames()[0].id, id);
}

#[test]
fn breed_spawns_on_the_penultimate_frame() {
    let (mut engine, parent) = breed_engine("10", 2);
    engine.tick(&on_screen());
    assert_eq!(engine.population(), 1);
    let before = engine.rng_draws();
    engine.tick(&on_screen());
    assert_eq!(engine.population(), 2);
    assert_eq!(engine.rng_draws(), before);
    engine.tick(&on_screen());
    let frames = engine.frames();
    let child = frames.iter().find(|frame| frame.id != parent).unwrap();
    assert_eq!(child.anchor, Point::new(110, 104));
    assert_eq!(child.behavior, "Child");
    assert!(!child.look_right);
}

fn breed_engine(born_x: &str, cap: usize) -> (Engine, shimeji_core::MascotId) {
    let split = act(
        "Split",
        Schema::English,
        ActionKind::Builtin(Builtin::Breed),
        &[
            ("BornX", born_x),
            ("BornY", "4"),
            ("BornBehavior", "Child"),
            ("BornCount", "1"),
        ],
        vec![pose("/breed.png", 0, 0, 2)],
        Vec::new(),
    );
    let child = act(
        "Child",
        Schema::English,
        ActionKind::Stay,
        &[],
        vec![pose("/child.png", 0, 0, 8)],
        Vec::new(),
    );
    let character = program(
        vec![("Split", split), ("Child", child)],
        vec![
            behavior("Split", "Split", 0, &[], None),
            behavior("Child", "Child", 0, &[], None),
        ],
        &[],
    );
    let mut engine = Engine::new(&character, 1);
    engine.set_mascot_cap(cap);
    engine.apply(&on_screen());
    let id = engine.spawn_behavior("Split").expect("breed behavior");
    engine.move_frozen(id, Point::new(100, 100));
    (engine, id)
}

fn on_screen() -> Snapshot {
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

#[test]
fn empty_pose_stay_stand_in_does_not_dispose() {
    let mut body = act(
        "Mystery",
        Schema::English,
        ActionKind::Stay,
        &[],
        Vec::new(),
        Vec::new(),
    );
    body.degraded_from = Some("com.example.Unknown".to_string());
    let character = program(
        vec![("Mystery", body)],
        vec![behavior("Mystery", "Mystery", 0, &[], None)],
        &[],
    );
    let mut engine = Engine::new(&character, 1);
    engine.apply(&on_screen());
    let id = engine.spawn_behavior("Mystery").unwrap();
    engine.move_frozen(id, Point::new(100, 100));
    engine.tick(&on_screen());
    engine.tick(&on_screen());
    assert_eq!(engine.population(), 1);
    let frame = &engine.frames()[0];
    assert_eq!(frame.behavior, "Mystery");
    assert!(frame.image.is_none());
}

#[test]
fn fast_follow_without_a_pose_does_not_dispose() {
    let body = act(
        "Scan",
        Schema::English,
        ActionKind::FastFollow(FastFollow::ScanMove),
        &[],
        Vec::new(),
        Vec::new(),
    );
    let character = program(
        vec![("Scan", body)],
        vec![behavior("Scan", "Scan", 0, &[], None)],
        &[],
    );
    let mut engine = Engine::new(&character, 1);
    engine.apply(&on_screen());
    let id = engine.spawn_behavior("Scan").unwrap();
    engine.move_frozen(id, Point::new(100, 100));
    engine.tick(&on_screen());
    engine.tick(&on_screen());
    assert_eq!(engine.population(), 1);
    assert_eq!(engine.frames()[0].behavior, "Scan");
}

#[test]
fn stay_with_no_matching_pose_still_disposes() {
    let mut body = act(
        "Sit",
        Schema::English,
        ActionKind::Stay,
        &[],
        vec![pose("/sit.png", 0, 0, 4)],
        Vec::new(),
    );
    body.animations[0].condition = Some(param("Condition", "#{false}").value);
    let character = program(
        vec![("Sit", body)],
        vec![behavior("Sit", "Sit", 0, &[], None)],
        &[],
    );
    let mut engine = Engine::new(&character, 1);
    engine.apply(&on_screen());
    let id = engine.spawn_behavior("Sit").unwrap();
    engine.move_frozen(id, Point::new(100, 100));
    engine.tick(&on_screen());
    engine.tick(&on_screen());
    assert_eq!(engine.population(), 0, "{id} should have been removed");
}

#[test]
fn fall_with_ie_ends_when_the_window_does_not_follow() {
    // The window's bottom-right corner is on its right wall, so `Fall.hasNext`
    // is false there. Shime hangs the anchor 64 px below that corner.
    let (mut engine, world) = ie_engine(
        Builtin::FallWithIe,
        &[("IeOffsetY", "-64")],
        Point::new(300, 314),
        ie_window(),
    );
    engine.tick(&world);
    let moves = drain(&mut engine);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].id, 1);
    assert_eq!((moves[0].x, moves[0].y), (100, 102));
    assert_eq!(engine.frames()[0].behavior, "Carry");
    engine.tick(&world);
    assert_eq!(engine.frames()[0].behavior, "Fall");
}

#[test]
fn fall_with_ie_keeps_the_corner_within_one_pixel() {
    let (mut engine, world) = ie_engine(
        Builtin::FallWithIe,
        &[("IeOffsetY", "-64")],
        Point::new(300, 314),
        ie_window(),
    );
    engine.tick(&world);
    let moves = drain(&mut engine);
    engine.push_window_event(shimeji_core::WindowEvent::Moved {
        id: moves[0].id,
        x: moves[0].x,
        y: moves[0].y + 1,
        width: moves[0].width,
        height: moves[0].height,
    });
    engine.tick(&world);
    assert_eq!(engine.frames()[0].behavior, "Carry");
    let again = drain(&mut engine);
    assert_eq!(again.len(), 1);
    // Second Fall tick: vy 2 -> 3.8, dy rounds to 5. Anchor 316+5, offset -64, height 150.
    assert_eq!(again[0].y, 107);
}

#[test]
fn walk_with_ie_requests_the_step_the_pose_took() {
    let (mut engine, world) = ie_engine(
        Builtin::WalkWithIe,
        &[("TargetX", "-1000")],
        Point::new(300, 250),
        ie_window(),
    );
    engine.tick(&world);
    let moves = drain(&mut engine);
    assert_eq!((moves[0].x, moves[0].y), (98, 100));
}

#[test]
fn throw_ie_moves_the_window_from_its_initial_velocity() {
    let mut world = ie_window();
    world.active_window = Some(WindowSample {
        visible: true,
        id: 0,
        x: 50,
        y: 80,
        width: 200,
        height: 150,
    });
    let (mut engine, world) = ie_engine(Builtin::ThrowIe, &[], Point::new(40, 40), world);
    engine.tick(&world);
    let moves = drain(&mut engine);
    assert_eq!((moves[0].x, moves[0].y), (18, 70));
    assert_eq!((moves[0].width, moves[0].height), (200, 150));
}

fn ie_window() -> Snapshot {
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
            id: 0,
            x: 100,
            y: 100,
            width: 200,
            height: 150,
        }),
        multiscreen: true,
    }
}

fn ie_engine(
    kind: Builtin,
    pairs: &[(&str, &str)],
    anchor: Point,
    world: Snapshot,
) -> (Engine, Snapshot) {
    let velocity_x = if kind == Builtin::WalkWithIe { -2 } else { 0 };
    let carry = act(
        "Carry",
        Schema::English,
        ActionKind::Builtin(kind),
        pairs,
        vec![pose("/ie.png", velocity_x, 0, 8)],
        Vec::new(),
    );
    let fall = act(
        "Fall",
        Schema::English,
        ActionKind::Builtin(Builtin::Fall),
        &[],
        vec![pose("/fall.png", 0, 0, 1)],
        Vec::new(),
    );
    let character = program(
        vec![("Carry", carry), ("Fall", fall)],
        vec![
            behavior("Carry", "Carry", 0, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = Engine::new(&character, 1);
    engine.apply(&world);
    let id = engine.spawn_behavior("Carry").expect("ie behavior");
    engine.move_frozen(id, anchor);
    (engine, world)
}

fn drain(engine: &mut Engine) -> Vec<shimeji_core::WindowMove> {
    let mut moves = Vec::new();
    let mut sounds = Vec::new();
    let mut spawned = Vec::new();
    engine.drain_effects(&mut moves, &mut sounds, &mut spawned);
    moves
}
