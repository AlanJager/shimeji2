//! Pose interjection at a tick boundary. An empty queue does not draw.

#[path = "support.rs"]
mod support;

use shimeji_core::model::{ActionKind, Builtin};
use shimeji_core::{MascotId, MouseButton};
use support::{behavior, program, start, stay, wide_screen};

fn pose(name: &str, kind: ActionKind, duration: &str) -> shimeji_core::model::Action {
    stay(name, kind, &[("Name", name), ("Duration", duration)])
}

fn walker() -> shimeji_core::model::Character {
    program(
        vec![
            ("Walk", pose("Walk", ActionKind::Stay, "100")),
            ("Wave", pose("Wave", ActionKind::Stay, "2")),
            ("ジャンプ", pose("ジャンプ", ActionKind::Stay, "2")),
            (
                "Drag",
                pose("Dragged", ActionKind::Builtin(Builtin::Dragged), "100"),
            ),
            ("Throw", pose("Thrown", ActionKind::Stay, "100")),
            ("Fall", pose("Fall", ActionKind::Stay, "100")),
        ],
        vec![
            behavior("Walk", "Walk", 100, &[], None),
            behavior("Wave", "Wave", 0, &[], None),
            behavior("Dragged", "Drag", 0, &[], None),
            behavior("Thrown", "Throw", 0, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    )
}

fn live() -> (shimeji_core::Engine, MascotId) {
    let character = walker();
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    let id = engine.spawn().unwrap();
    engine.tick(&wide_screen());
    (engine, id)
}

fn names_for(engine: &mut shimeji_core::Engine, id: MascotId, ticks: u32) -> Vec<String> {
    let mut names = Vec::new();
    for _ in 0..ticks {
        engine.tick(&wide_screen());
        names.push(support::behavior_name(engine, id));
    }
    names
}

#[test]
fn interject_is_skipped_while_dragged() {
    let (mut engine, id) = live();
    engine.press(Some(id), MouseButton::Left);
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Dragged");
    engine.interject(id, "Wave");
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Dragged");
}

#[test]
fn interject_is_skipped_while_thrown() {
    let (mut engine, id) = live();
    engine.set_behavior(id, "Thrown");
    assert_eq!(support::behavior_name(&engine, id), "Thrown");
    engine.interject(id, "Wave");
    engine.tick(&wide_screen());
    assert_eq!(support::behavior_name(&engine, id), "Thrown");
}

#[test]
fn interject_runs_once_while_walking_then_selection_resumes() {
    let (mut engine, id) = live();
    assert_eq!(support::behavior_name(&engine, id), "Walk");
    engine.interject(id, "Wave");
    let names = names_for(&mut engine, id, 8);
    assert_eq!(names[0], "Wave", "{names:?}");
    assert!(
        names.iter().any(|name| name == "Walk"),
        "selection did not resume: {names:?}"
    );
    let resumed = names.iter().position(|name| name == "Walk").unwrap();
    assert!(names[resumed..].iter().all(|name| name == "Walk"));
}

#[test]
fn an_action_name_runs_once_then_selection_resumes() {
    let (mut engine, id) = live();
    assert_eq!(support::behavior_name(&engine, id), "Walk");
    engine.interject(id, "ジャンプ");
    let names = names_for(&mut engine, id, 8);
    assert_eq!(names[0], "ジャンプ", "{names:?}");
    assert!(
        names.iter().any(|name| name == "Walk"),
        "selection did not resume: {names:?}"
    );
}
