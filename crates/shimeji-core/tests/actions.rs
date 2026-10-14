//! Action lifecycle, from the vendored `Move`, `Stay`, `Animate`, `Sequence`,
//! and `Select` sources.

use shimeji_core::action::ActionError;
use shimeji_core::env::Point;
use shimeji_core::model::{
    Action, ActionKind, ActionNode, ActionRef, Animation, Builtin, Character, Information,
    OrderedMap, Param, Pose, Schema, ScriptSource,
};
use shimeji_core::{JavaRandom, Mascot, compile_character};

fn param(key: &str, raw: &str) -> Param {
    let (value, error) = ScriptSource::parse_attribute(raw);
    assert!(error.is_none(), "{raw} failed to parse");
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

fn action(
    name: Option<&str>,
    kind: ActionKind,
    pairs: &[(&str, &str)],
    poses: Vec<Pose>,
    children: Vec<ActionNode>,
) -> Action {
    Action {
        name: name.map(str::to_string),
        schema: Schema::English,
        kind,
        degraded_from: None,
        params: params(pairs),
        animations: if poses.is_empty() {
            Vec::new()
        } else {
            vec![animation(poses)]
        },
        children,
        line: 1,
    }
}

fn character(actions: Vec<(&str, Action)>) -> Character {
    let mut map = OrderedMap::new();
    for (name, action) in actions {
        map.insert(name, action);
    }
    Character {
        name: "test".to_string(),
        schema: Schema::English,
        behavior_schema: Schema::English,
        image_root: String::new(),
        actions_file: "actions.xml".to_string(),
        behaviors_file: "behaviors.xml".to_string(),
        info_file: None,
        constants: OrderedMap::new(),
        actions: map,
        behaviors: OrderedMap::new(),
        behavior_tree: Vec::new(),
        info: Information::default(),
    }
}

fn step(
    action: &mut shimeji_core::RunningAction,
    mascot: &mut Mascot,
    rng: &mut JavaRandom,
) -> Result<bool, ActionError> {
    if !action.has_next(mascot, rng)? {
        return Ok(false);
    }
    action.next(mascot, rng)?;
    mascot.time += 1;
    Ok(true)
}

fn number(value: shimeji_core::Value) -> f64 {
    value.as_number().expect("number")
}

#[test]
fn move_with_only_target_x_stops_at_x() {
    let program = compile_character(&character(vec![(
        "Go",
        action(
            Some("Go"),
            ActionKind::Move,
            &[("TargetX", "10")],
            vec![pose("/walk.png", -2, 0, 1)],
            Vec::new(),
        ),
    )]));
    let mut action = program.instantiate("Go").unwrap();
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();

    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.anchor.x, 2);
    assert!(action.has_next(&mut mascot, &mut rng).unwrap());

    while step(&mut action, &mut mascot, &mut rng).unwrap() {}
    assert_eq!(mascot.anchor, Point::new(10, 0));
    assert!(mascot.look_right);
    assert!(!action.has_next(&mut mascot, &mut rng).unwrap());
}

#[test]
fn move_with_both_targets_stops_when_every_axis_arrives() {
    // Vendored `Move.hasNext` keeps going while either configured axis is short.
    // It stops only once both have been reached. Only-X still stops at X.
    let program = compile_character(&character(vec![(
        "Go",
        action(
            Some("Go"),
            ActionKind::Move,
            &[("TargetX", "4"), ("TargetY", "10")],
            vec![pose("/walk.png", -2, 2, 1)],
            Vec::new(),
        ),
    )]));
    let mut action = program.instantiate("Go").unwrap();
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();

    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.anchor.x, 4);
    assert_eq!(mascot.anchor.y, 4);
    assert!(action.has_next(&mut mascot, &mut rng).unwrap());

    while step(&mut action, &mut mascot, &mut rng).unwrap() {}
    assert_eq!(mascot.anchor, Point::new(4, 10));
    assert!(!action.has_next(&mut mascot, &mut rng).unwrap());
}

#[test]
fn select_skips_a_child_whose_condition_is_false() {
    let skip = action(
        None,
        ActionKind::Stay,
        &[("Condition", "false")],
        vec![pose("/a.png", 0, 0, 1)],
        Vec::new(),
    );
    let keep = action(
        None,
        ActionKind::Stay,
        &[],
        vec![pose("/b.png", 0, 0, 1)],
        Vec::new(),
    );
    let program = compile_character(&character(vec![(
        "Pick",
        action(
            Some("Pick"),
            ActionKind::Select,
            &[],
            Vec::new(),
            vec![ActionNode::Action(skip), ActionNode::Action(keep)],
        ),
    )]));
    let mut action = program.instantiate("Pick").unwrap();
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();
    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.image_key.as_deref(), Some("/b.png"));
}

#[test]
fn select_does_not_advance_when_the_chosen_child_ends() {
    let first = action(
        None,
        ActionKind::Stay,
        &[("Duration", "1")],
        vec![pose("/a.png", 0, 0, 1)],
        Vec::new(),
    );
    let second = action(
        None,
        ActionKind::Stay,
        &[("Duration", "1")],
        vec![pose("/b.png", 0, 0, 1)],
        Vec::new(),
    );
    let program = compile_character(&character(vec![(
        "Pick",
        action(
            Some("Pick"),
            ActionKind::Select,
            &[],
            Vec::new(),
            vec![ActionNode::Action(first), ActionNode::Action(second)],
        ),
    )]));
    let mut action = program.instantiate("Pick").unwrap();
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();
    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.image_key.as_deref(), Some("/a.png"));
    assert!(!action.has_next(&mut mascot, &mut rng).unwrap());
}

#[test]
fn sequence_loop_wraps_to_the_first_child() {
    let child = |image: &str| {
        action(
            None,
            ActionKind::Stay,
            &[("Duration", "1")],
            vec![pose(image, 0, 0, 1)],
            Vec::new(),
        )
    };
    let program = compile_character(&character(vec![(
        "Loop",
        action(
            Some("Loop"),
            ActionKind::Sequence,
            &[("Loop", "true")],
            Vec::new(),
            vec![
                ActionNode::Action(child("/a.png")),
                ActionNode::Action(child("/b.png")),
            ],
        ),
    )]));
    let mut action = program.instantiate("Loop").unwrap();
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();

    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.image_key.as_deref(), Some("/a.png"));
    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.image_key.as_deref(), Some("/b.png"));
    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.image_key.as_deref(), Some("/a.png"));
}

#[test]
fn hash_reevaluates_after_next_and_dollar_does_not() {
    let program = compile_character(&character(vec![(
        "Watch",
        action(
            Some("Watch"),
            ActionKind::Stay,
            &[
                ("once", "${mascot.anchor.x}"),
                ("every", "#{mascot.anchor.x}"),
            ],
            vec![pose("/still.png", 0, 0, 8)],
            Vec::new(),
        ),
    )]));
    let mut action = program.instantiate("Watch").unwrap();
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();

    assert_eq!(number(action.read("once", &mascot, &mut rng).unwrap()), 0.0);
    assert_eq!(
        number(action.read("every", &mascot, &mut rng).unwrap()),
        0.0
    );
    mascot.anchor.x = 7;
    assert_eq!(number(action.read("once", &mascot, &mut rng).unwrap()), 0.0);
    assert_eq!(
        number(action.read("every", &mascot, &mut rng).unwrap()),
        0.0
    );

    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(number(action.read("once", &mascot, &mut rng).unwrap()), 0.0);
    assert_eq!(
        number(action.read("every", &mascot, &mut rng).unwrap()),
        7.0
    );
}

#[test]
fn pose_velocity_negates_x_when_look_right() {
    let program = compile_character(&character(vec![(
        "Sit",
        action(
            Some("Sit"),
            ActionKind::Stay,
            &[],
            vec![pose("/walk.png", -2, 0, 4)],
            Vec::new(),
        ),
    )]));
    let mut rng = JavaRandom::new(1);

    let mut facing_left = program.instantiate("Sit").unwrap();
    let mut mascot = Mascot::new();
    facing_left.init(&mut mascot, &mut rng).unwrap();
    assert!(step(&mut facing_left, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.anchor.x, -2);

    let mut facing_right = program.instantiate("Sit").unwrap();
    let mut mascot = Mascot::new();
    mascot.look_right = true;
    facing_right.init(&mut mascot, &mut rng).unwrap();
    assert!(step(&mut facing_right, &mut mascot, &mut rng).unwrap());
    assert_eq!(mascot.anchor.x, 2);
}

#[test]
fn animate_ends_after_one_cycle() {
    let program = compile_character(&character(vec![(
        "Once",
        action(
            Some("Once"),
            ActionKind::Animate,
            &[],
            vec![pose("/a.png", 0, 0, 2)],
            Vec::new(),
        ),
    )]));
    let mut action = program.instantiate("Once").unwrap();
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();
    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert!(action.has_next(&mut mascot, &mut rng).unwrap());
    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert!(!action.has_next(&mut mascot, &mut rng).unwrap());
}

#[test]
fn builtin_fall_gains_two_pixels_on_the_first_tick() {
    let program = compile_character(&character(vec![(
        "Drop",
        action(
            Some("Drop"),
            ActionKind::Builtin(Builtin::Fall),
            &[],
            vec![pose("/fall.png", 0, 0, 1)],
            Vec::new(),
        ),
    )]));
    let mut action = program.instantiate("Drop").unwrap();
    let mut mascot = Mascot::new();
    mascot.anchor = Point::new(3, 8);
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();
    assert!(step(&mut action, &mut mascot, &mut rng).unwrap());
    assert_eq!(action.class_name(), "Fall");
    assert_eq!(mascot.image_key.as_deref(), Some("/fall.png"));
    // First Fall tick from rest: gravity 2, pose velocity 0, so y moves by +2.
    assert_eq!(mascot.anchor, Point::new(3, 10));
}

#[test]
fn action_reference_params_overlay_the_referenced_action() {
    let go = action(
        Some("Go"),
        ActionKind::Move,
        &[("TargetX", "100")],
        vec![pose("/walk.png", -2, 0, 1)],
        Vec::new(),
    );
    let mut overlay = OrderedMap::new();
    overlay.insert("TargetX", param("TargetX", "4"));
    let reference = ActionNode::Reference(ActionRef {
        name: "Go".to_string(),
        params: overlay,
        line: 1,
    });
    let program = compile_character(&character(vec![
        ("Go", go),
        (
            "Wrap",
            action(
                Some("Wrap"),
                ActionKind::Sequence,
                &[],
                Vec::new(),
                vec![reference],
            ),
        ),
    ]));
    let mut action = program.instantiate("Wrap").unwrap();
    let mut mascot = Mascot::new();
    let mut rng = JavaRandom::new(1);
    action.init(&mut mascot, &mut rng).unwrap();
    while step(&mut action, &mut mascot, &mut rng).unwrap() {}
    assert_eq!(mascot.anchor.x, 4);
}
