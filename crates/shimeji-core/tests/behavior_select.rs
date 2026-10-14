//! Behavior selection, from `Configuration.buildNextBehavior` and `UserBehavior`.

#[path = "support.rs"]
mod support;

use shimeji_core::diagnostics::DiagnosticKind;
use shimeji_core::model::ActionKind;
use shimeji_core::{JavaRandom, MascotId};
use support::{
    behavior, behavior_lists, desktop, list, program, reference, start, stay, wide_screen,
    window_at,
};

fn fall_action() -> shimeji_core::model::Action {
    stay("Fall", ActionKind::Stay, &[("Name", "Fall")])
}

fn named(kind_name: &str, duration: &str) -> shimeji_core::model::Action {
    stay(
        kind_name,
        ActionKind::Stay,
        &[("Name", kind_name), ("Duration", duration)],
    )
}

fn admit(engine: &mut shimeji_core::Engine, snapshot: &shimeji_core::env::Snapshot) -> MascotId {
    engine.apply(snapshot);
    engine.spawn().expect("spawn")
}

#[test]
fn add_false_replaces_and_add_true_includes_the_ref() {
    let character = program(
        vec![
            ("A", named("A", "1")),
            ("B", named("B", "100")),
            ("C", named("C", "100")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior(
                "A",
                "A",
                100,
                &[],
                Some((false, vec![reference("B", 100, &[])])),
            ),
            behavior("B", "B", 0, &[], None),
            behavior("C", "C", 50, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let id = admit(&mut engine, &wide_screen());
    engine.tick(&wide_screen());
    let weights = engine.candidate_weights(id).expect("weights");
    assert_eq!(weights, vec![("B".to_string(), 100)]);

    let character = program(
        vec![
            ("A", named("A", "1")),
            ("B", named("B", "100")),
            ("C", named("C", "100")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior(
                "A",
                "A",
                100,
                &[],
                Some((true, vec![reference("B", 5, &[])])),
            ),
            behavior("B", "B", 0, &[], None),
            behavior("C", "C", 50, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let id = admit(&mut engine, &wide_screen());
    engine.tick(&wide_screen());
    let weights = engine.candidate_weights(id).expect("weights");
    assert_eq!(
        weights,
        vec![
            ("A".to_string(), 100),
            ("C".to_string(), 50),
            ("B".to_string(), 5),
        ]
    );
}

#[test]
fn last_next_list_add_wins() {
    let character = program(
        vec![
            ("A", named("A", "100")),
            ("X", named("X", "100")),
            ("Y", named("Y", "100")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior_lists(
                "A",
                "A",
                100,
                vec![
                    list(false, vec![reference("X", 3, &[])]),
                    list(true, vec![reference("Y", 7, &[])]),
                ],
            ),
            behavior("X", "X", 0, &[], None),
            behavior("Y", "Y", 0, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let id = admit(&mut engine, &wide_screen());
    engine.tick(&wide_screen());
    let weights = engine.candidate_weights(id).expect("weights");
    assert!(
        weights
            .iter()
            .any(|(name, freq)| name == "A" && *freq == 100)
    );
    assert!(weights.iter().any(|(name, freq)| name == "X" && *freq == 3));
    assert!(weights.iter().any(|(name, freq)| name == "Y" && *freq == 7));

    let character = program(
        vec![
            ("A", named("A", "100")),
            ("X", named("X", "100")),
            ("Y", named("Y", "100")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior_lists(
                "A",
                "A",
                100,
                vec![
                    list(true, vec![reference("X", 3, &[])]),
                    list(false, vec![reference("Y", 7, &[])]),
                ],
            ),
            behavior("X", "X", 0, &[], None),
            behavior("Y", "Y", 0, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let id = admit(&mut engine, &wide_screen());
    engine.tick(&wide_screen());
    let weights = engine.candidate_weights(id).expect("weights");
    assert_eq!(weights, vec![("X".to_string(), 3), ("Y".to_string(), 7)]);
}

#[test]
fn weighted_picks_match_a_fixed_seed() {
    let character = program(
        vec![
            ("A", named("A", "1")),
            ("B", named("B", "1")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior("A", "A", 1, &[], None),
            behavior("B", "B", 3, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let id = admit(&mut engine, &wide_screen());
    let mut shadow = JavaRandom::new(1);
    let _facing = shadow.next_double();
    let mut expect = Vec::new();
    expect.push(shadow_pick(&mut shadow));
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), expect[0]);
    for _ in 0..20 {
        expect.push(shadow_pick(&mut shadow));
        engine.tick(&wide_screen());
        assert_eq!(
            support::behavior_name(&engine, id),
            *expect.last().expect("pick")
        );
    }
    assert!(expect.contains(&"A"));
    assert!(expect.contains(&"B"));
}

fn shadow_pick(rng: &mut JavaRandom) -> &'static str {
    let mut random = rng.next_double() * 4.0;
    random -= 1.0;
    if random < 0.0 { "A" } else { "B" }
}

#[test]
fn no_candidate_teleports_above_the_screen_and_falls() {
    let character = program(
        vec![("Fall", fall_action())],
        vec![behavior("Fall", "Fall", 0, &[], None)],
        &[],
    );
    let mut engine = start(&character);
    engine.apply(&desktop());
    let id = engine.spawn().expect("spawn");
    let mut shadow = JavaRandom::new(1);
    let look_right = shadow.next_double() < 0.5;
    let x = (shadow.next_double() * f64::from(1280 - 2)) as i32 + 1;
    engine.tick(&desktop());
    let frame = engine
        .frames()
        .into_iter()
        .find(|frame| frame.id == id)
        .unwrap();
    assert_eq!(frame.behavior, "Fall");
    assert_eq!(frame.anchor.x, x);
    assert_eq!(frame.anchor.y, -256);
    assert_eq!(frame.look_right, look_right);
    assert_eq!(engine.rng_draws(), 2);
}

#[test]
fn lost_ground_switches_to_fall() {
    let character = program(
        vec![
            (
                "Sit",
                stay(
                    "Sit",
                    ActionKind::Stay,
                    &[
                        ("Name", "Sit"),
                        ("BorderType", "Floor"),
                        ("Duration", "100"),
                    ],
                ),
            ),
            ("Fall", fall_action()),
        ],
        vec![
            behavior("Sit", "Sit", 100, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let on_floor = window_at(0, 100);
    engine.apply(&on_floor);
    let id = engine
        .spawn_at(shimeji_core::env::Point::new(20, 100))
        .unwrap();
    engine.tick(&on_floor);
    let frame = engine
        .frames()
        .into_iter()
        .find(|frame| frame.id == id)
        .unwrap();
    assert_eq!(frame.behavior, "Sit");
    assert_eq!(frame.anchor.y, 100);
    engine.tick(&window_at(0, 200));
    let frame = engine
        .frames()
        .into_iter()
        .find(|frame| frame.id == id)
        .unwrap();
    assert_eq!(frame.behavior, "Fall");
}

#[test]
fn group_condition_math_random_is_drawn_once_per_behavior() {
    let character = program(
        vec![
            ("A", named("A", "100")),
            ("B", named("B", "100")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior("A", "A", 100, &["#{Math.random() < 2}"], None),
            behavior("B", "B", 1, &["#{Math.random() < 2}"], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    engine.spawn().unwrap();
    assert_eq!(engine.rng_draws(), 4);
}

#[test]
fn duplicate_candidates_are_kept() {
    let character = program(
        vec![("Twin", named("Twin", "100")), ("Fall", fall_action())],
        vec![
            behavior(
                "Twin",
                "Twin",
                1,
                &[],
                Some((true, vec![reference("Twin", 3, &[])])),
            ),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let id = admit(&mut engine, &wide_screen());
    engine.tick(&wide_screen());
    let weights = engine.candidate_weights(id).unwrap();
    assert_eq!(
        weights,
        vec![("Twin".to_string(), 1), ("Twin".to_string(), 3)]
    );
}

#[test]
fn behavior_condition_error_is_false_and_keeps_the_mascot() {
    let character = program(
        vec![
            ("Boom", named("Boom", "100")),
            ("Stand", named("Stand", "100")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior("Boom", "Boom", 100, &["#{null.foo}"], None),
            behavior("Stand", "Stand", 1, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    let id = engine.spawn().unwrap();
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Stand");
    assert_eq!(engine.frames()[0].anchor.x, -4000);
    assert!(
        engine
            .diagnostics()
            .iter()
            .any(|diag| diag.kind == DiagnosticKind::Expression)
    );
}

#[test]
fn action_parameter_error_disposes_on_the_next_tick() {
    let character = program(
        vec![
            ("Stand", named("Stand", "3")),
            ("Bad", named("Bad", "#{null.foo}")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior(
                "Stand",
                "Stand",
                100,
                &[],
                Some((false, vec![reference("Bad", 100, &[])])),
            ),
            behavior("Bad", "Bad", 0, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let id = admit(&mut engine, &wide_screen());
    for _ in 0..3 {
        engine.tick(&wide_screen());
        assert_eq!(support::behavior_name(&engine, id), "Stand");
    }
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Bad");
    assert_eq!(engine.rng_draws(), 3);
    engine.tick(&wide_screen());
    assert!(engine.frames().is_empty());
    assert!(
        engine
            .diagnostics()
            .iter()
            .any(|diag| diag.kind == DiagnosticKind::Expression)
    );
}

#[test]
fn non_boolean_condition_is_false_and_the_tick_continues() {
    let character = program(
        vec![
            ("Stand", named("Stand", "1")),
            ("Other", named("Other", "100")),
            ("Fall", fall_action()),
        ],
        vec![
            behavior(
                "Stand",
                "Stand",
                100,
                &[],
                Some((false, vec![reference("Other", 100, &["#{1}"])])),
            ),
            behavior("Other", "Other", 0, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    let id = admit(&mut engine, &wide_screen());
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Stand");
    engine.tick(&wide_screen());
    let frame = &engine.frames()[0];
    assert_eq!(frame.id, id);
    assert_eq!(frame.behavior, "Fall");
    assert!(engine.diagnostics().iter().any(|diag| {
        diag.kind == DiagnosticKind::Expression && diag.message.contains("boolean")
    }));
}

#[test]
fn constants_are_visible_to_behavior_conditions_and_do_not_hide_mascot() {
    let character = program(
        vec![("Open", named("Open", "100")), ("Fall", fall_action())],
        vec![
            behavior("Open", "Open", 100, &["#{maxCount == 0}"], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[("maxCount", "0"), ("mascot", "nope")],
    );
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    let id = engine.spawn().unwrap();
    engine.tick(&wide_screen());
    let frame = &engine.frames()[0];
    assert_eq!(frame.id, id);
    assert_eq!(frame.behavior, "Open");
    assert_eq!(frame.anchor.x, -4000);
}

#[test]
fn total_count_gate_uses_the_live_population() {
    let character = program(
        vec![("Gate", named("Gate", "100")), ("Fall", fall_action())],
        vec![
            behavior(
                "Gate",
                "Gate",
                100,
                &["#{mascot.totalCount < maxCount}"],
                None,
            ),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[("maxCount", "1")],
    );
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    let first = engine.spawn().unwrap();
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, first), "Gate");
    let second = engine.spawn().unwrap();
    engine.tick(&wide_screen());
    let frames = engine.frames();
    let second_frame = frames.iter().find(|frame| frame.id == second).unwrap();
    assert_eq!(second_frame.behavior, "Fall");
    assert_ne!(second_frame.anchor.y, -4000);
}

#[test]
fn sitting_mascot_second_tick_changes_neither_anchor_nor_image() {
    let character = program(
        vec![
            (
                "Sit",
                stay(
                    "Sit",
                    ActionKind::Stay,
                    &[("Name", "Sit"), ("Duration", "100")],
                ),
            ),
            ("Fall", fall_action()),
        ],
        vec![
            behavior("Sit", "Sit", 100, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    engine.apply(&desktop());
    engine
        .spawn_at(shimeji_core::env::Point::new(200, 200))
        .unwrap();
    let first = engine.tick(&desktop());
    assert!(first.mascots[0].image_changed);
    assert!(!first.mascots[0].anchor_changed);
    let second = engine.tick(&desktop());
    assert!(!second.mascots[0].anchor_changed);
    assert!(!second.mascots[0].image_changed);
}
