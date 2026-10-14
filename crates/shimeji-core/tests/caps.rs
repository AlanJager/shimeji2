//! Live-mascot cap and the shared per-tick condition budget.

#[path = "support.rs"]
mod support;

use std::time::Instant;

use shimeji_core::DEFAULT_EVAL_BUDGET;
use shimeji_core::diagnostics::DiagnosticKind;
use shimeji_core::model::{ActionKind, ScriptSource};
use support::{behavior, program, reference, start, stay, wide_screen};

fn body(name: &str, duration: &str) -> shimeji_core::model::Action {
    stay(
        name,
        ActionKind::Stay,
        &[("Name", name), ("Duration", duration)],
    )
}

#[test]
fn cap_refuses_the_spawn_and_draws_nothing() {
    let character = program(
        vec![("Sit", body("Sit", "100")), ("Fall", body("Fall", "100"))],
        vec![
            behavior("Sit", "Sit", 100, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    engine.set_mascot_cap(1);
    engine.apply(&wide_screen());
    assert!(engine.spawn().is_some());
    let draws = engine.rng_draws();
    assert!(engine.spawn().is_none());
    assert_eq!(engine.rng_draws(), draws);
    assert_eq!(engine.population(), 1);
    assert!(
        engine.diagnostics().iter().any(|diag| {
            diag.kind == DiagnosticKind::Other && diag.message.contains("mascot cap")
        })
    );
}

#[test]
fn ten_thousand_conditions_stop_at_the_budget() {
    let mut heavy = Vec::new();
    for _ in 0..10_000 {
        let (value, error) = ScriptSource::parse_attribute("#{Math.random() < 2}");
        assert!(error.is_none());
        heavy.push(value);
    }
    let mut reference = reference("Heavy", 100, &[]);
    reference.conditions = heavy;
    let character = program(
        vec![
            ("Sit", body("Sit", "1")),
            ("Heavy", body("Heavy", "100")),
            ("Fall", body("Fall", "100")),
        ],
        vec![
            behavior("Sit", "Sit", 100, &[], Some((false, vec![reference]))),
            behavior("Heavy", "Heavy", 0, &[], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    engine.spawn().unwrap();
    engine.tick(&wide_screen());
    let started = Instant::now();
    engine.tick(&wide_screen());
    assert!(started.elapsed().as_secs() < 2);
    assert_eq!(engine.rng_draws(), 2 + u64::from(DEFAULT_EVAL_BUDGET) + 1);
    assert!(engine.diagnostics().iter().any(|diag| {
        diag.kind == DiagnosticKind::Other && diag.message.contains("evaluation budget")
    }));
    assert_eq!(engine.frames()[0].behavior, "Fall");
}

#[test]
fn a_tripped_budget_rotates_the_next_tick() {
    let character = program(
        vec![
            ("A", body("A", "1")),
            ("B", body("B", "1")),
            ("Fall", body("Fall", "1")),
        ],
        vec![
            behavior("A", "A", 100, &["#{Math.random() < 2}"], None),
            behavior("B", "B", 100, &["#{Math.random() < 2}"], None),
            behavior("Fall", "Fall", 0, &[], None),
        ],
        &[],
    );
    let mut engine = start(&character);
    engine.apply(&wide_screen());
    engine.spawn().unwrap();
    engine.spawn().unwrap();
    engine.set_eval_budget(1);
    engine.tick(&wide_screen());
    engine.tick(&wide_screen());
    let frames = engine.frames();
    assert_eq!(frames[0].behavior, "A");
    assert_eq!(frames[1].behavior, "Fall");
    engine.tick(&wide_screen());
    let frames = engine.frames();
    assert_eq!(frames[0].behavior, "Fall");
    assert_ne!(frames[1].behavior, "Fall");
}
