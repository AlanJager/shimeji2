//! Pointer events at a tick boundary, and `move_frozen` outside the tick.

#[path = "support.rs"]
mod support;

use shimeji_core::diagnostics::DiagnosticKind;
use shimeji_core::env::Point;
use shimeji_core::model::{ActionKind, Builtin};
use shimeji_core::{MascotId, MouseButton};
use support::{behavior, program, start, stay, wide_screen};

fn pose_action(
    name: &str,
    kind: ActionKind,
    pairs: &[(&str, &str)],
) -> shimeji_core::model::Action {
    stay(name, kind, pairs)
}

fn drag_character(draggable: Option<&str>) -> shimeji_core::model::Character {
    let mut sit_pairs = vec![("Name", "Sit"), ("Duration", "100")];
    if let Some(raw) = draggable {
        sit_pairs.push(("Draggable", raw));
    }
    program(
        vec![
            ("Sit", pose_action("Sit", ActionKind::Stay, &sit_pairs)),
            (
                "Drag",
                pose_action(
                    "Drag",
                    ActionKind::Builtin(Builtin::Dragged),
                    &[("Name", "Dragged"), ("Duration", "100")],
                ),
            ),
            (
                "Throw",
                pose_action(
                    "Throw",
                    ActionKind::Stay,
                    &[("Name", "Thrown"), ("Duration", "100")],
                ),
            ),
            (
                "Fall",
                pose_action(
                    "Fall",
                    ActionKind::Stay,
                    &[("Name", "Fall"), ("Duration", "100")],
                ),
            ),
        ],
        vec![
            behavior("Sit", "Sit", 100, &[], None),
            behavior("Dragged", "Drag", 0, &[], None),
            behavior("Thrown", "Throw", 0, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    )
}

fn sitting() -> (shimeji_core::Engine, MascotId) {
    let character = drag_character(None);
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    let id = engine.spawn().unwrap();
    engine.tick(&wide_screen());
    (engine, id)
}

#[test]
fn press_then_a_later_release_throws() {
    let (mut engine, id) = sitting();
    engine.press(Some(id), MouseButton::Left);
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Dragged");
    engine.release(Some(id), MouseButton::Left);
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Thrown");
}

#[test]
fn same_tick_release_does_not_throw() {
    let (mut engine, id) = sitting();
    engine.press(Some(id), MouseButton::Left);
    engine.release(Some(id), MouseButton::Left);
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Dragged");
}

#[test]
fn draggable_false_ignores_the_press() {
    let character = drag_character(Some("false"));
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    let id = engine.spawn().unwrap();
    engine.tick(&wide_screen());
    engine.press(None, MouseButton::Left);
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Sit");
}

#[test]
fn right_button_and_unknown_target_are_ignored() {
    let (mut engine, id) = sitting();
    engine.press(None, MouseButton::Right);
    engine.press(Some(MascotId(99)), MouseButton::Left);
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Sit");
}

#[test]
fn draggable_error_removes_the_mascot_on_that_tick() {
    let character = drag_character(Some("#{null.foo}"));
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    engine.spawn().unwrap();
    engine.tick(&wide_screen());
    engine.press(None, MouseButton::Left);
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
fn move_frozen_sets_the_anchor_outside_the_tick() {
    let character = drag_character(None);
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    let id = engine.spawn().unwrap();
    let delta = engine.move_frozen(id, Point::new(5, 6)).unwrap();
    assert!(delta.anchor_changed);
    assert!(!delta.image_changed);
    let tick = engine.tick(&wide_screen());
    assert!(!tick.mascots[0].anchor_changed);
    assert_eq!(engine.frames()[0].anchor, Point::new(5, 6));
    let again = engine.move_frozen(id, Point::new(8, 9)).unwrap();
    assert!(again.anchor_changed);
    let next = engine.tick(&wide_screen());
    assert!(!next.mascots[0].anchor_changed);
    assert_eq!(engine.frames()[0].anchor, Point::new(8, 9));
}
