//! Committed golden traces vs the engine.
//!
//! Twelve goldens must be tick-identical. `ee-error-nonboolean` is the
//! documented exception: the reference aborts the tick with a ClassCastException,
//! and this engine treats a non-Boolean as false and continues.

use std::fs;
use std::path::PathBuf;

use shimeji_core::diagnostics::DiagnosticKind;
use shimeji_core::model::{Character, Pack};
use shimeji_core::testing::{Scenario, first_divergence, run_output};
use shimeji_packs::load_dir;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Case {
    name: &'static str,
    pack: &'static str,
}

const CASES: &[Case] = &[
    Case {
        name: "shime-floor-only",
        pack: "assets/packs/shime",
    },
    Case {
        name: "shime-drag-throw",
        pack: "assets/packs/shime",
    },
    Case {
        name: "shime-breed-limit",
        pack: "assets/packs/shime",
    },
    Case {
        name: "shime-moving-window",
        pack: "assets/packs/shime",
    },
    Case {
        name: "shime-offscreen-teleport",
        pack: "assets/packs/shime",
    },
    Case {
        name: "shime-two-screens",
        pack: "assets/packs/shime",
    },
    Case {
        name: "shime-window-drag-up",
        pack: "assets/packs/shime",
    },
    Case {
        name: "ee-error-action",
        pack: "tests/fixtures/ee-synthetic/ee-error-action",
    },
    Case {
        name: "ee-error-condition",
        pack: "tests/fixtures/ee-synthetic/ee-error-condition",
    },
    Case {
        name: "ee-error-nonboolean",
        pack: "tests/fixtures/ee-synthetic/ee-error-nonboolean",
    },
    Case {
        name: "ee-layout-dropped",
        pack: "tests/fixtures/ee-synthetic/ee-layout-dropped",
    },
    Case {
        name: "ee-layout-root",
        pack: "tests/fixtures/ee-synthetic/ee-layout-root",
    },
    Case {
        name: "ee-layout-set",
        pack: "tests/fixtures/ee-synthetic/ee-layout-set",
    },
];

const NONBOOLEAN: &str = "ee-error-nonboolean";
const NONBOOLEAN_TICK1: &str = "T\t1\tmascot1\tFall\tFall\t-3254\t-256\t/body.png\tfalse\t3";
const NONBOOLEAN_ABORT: &str = "# abort\t1\tjava.lang.ClassCastException\tclass java.lang.Integer cannot be cast to class java.lang.Boolean (java.lang.Integer and java.lang.Boolean are in module java.base of loader 'bootstrap')";

fn character<'a>(pack: &'a Pack, scenario: &Scenario) -> &'a Character {
    if let Some(found) = pack.character(&scenario.image_set) {
        return found;
    }
    if pack.characters.len() == 1 {
        return &pack.characters[0];
    }
    let names: Vec<&str> = pack
        .characters
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    panic!(
        "image set {} is not in the pack (have {})",
        scenario.image_set,
        names.join(", ")
    );
}

fn golden_path(name: &str) -> PathBuf {
    let dir = if name.starts_with("ee-") {
        "ee-synthetic"
    } else {
        "shime"
    };
    root().join(format!("tests/golden/{dir}/{name}.trace"))
}

fn budget_trips(diagnostics: &[shimeji_core::diagnostics::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|diag| {
            diag.kind == DiagnosticKind::Other
                && diag.message.contains("evaluation budget exhausted")
        })
        .count()
}

/// A wrong physics constant (anchor y off by 2 px) reports the tick and both lines.
#[test]
fn first_divergence_ignores_headers_and_reports_the_tick() {
    let actual = "# scenario\tlocal\nT\t0\tmascot1\tFall\tFall\t1\t-256\t/a.png\tfalse\t2\n";
    let expected = "# scenario\tgolden\nT\t0\tmascot1\tFall\tFall\t1\t-254\t/a.png\tfalse\t2\n";
    let found = first_divergence(actual, expected).expect("diverges");
    assert_eq!(found.tick, 0);
    assert!(found.actual.contains("-256"));
    assert!(found.expected.contains("-254"));
}

#[test]
fn committed_goldens_match_except_the_nonboolean_abort() {
    let mut failures = Vec::new();
    let mut rows = Vec::new();
    for case in CASES {
        let loaded = load_dir(root().join(case.pack)).expect(case.pack);
        let text =
            fs::read_to_string(root().join(format!("tests/scenarios/{}.json", case.name))).unwrap();
        let scenario = Scenario::parse(&text).expect(case.name);
        let character = character(&loaded.pack, &scenario);
        let output = run_output(character, &scenario);
        let golden = fs::read_to_string(golden_path(case.name)).unwrap();
        let trips = budget_trips(&output.diagnostics);
        if trips != 0 {
            failures.push(format!(
                "{}: evaluation budget tripped {trips} time(s)",
                case.name
            ));
        }
        if case.name == NONBOOLEAN {
            match check_nonboolean(&output.trace, &golden) {
                Ok(()) => rows.push(format!(
                    "{}\texception\ttick 1 abort vs continue",
                    case.name
                )),
                Err(message) => {
                    rows.push(format!("{}\tFAIL\t{message}", case.name));
                    failures.push(format!("{}: {message}", case.name));
                }
            }
            continue;
        }
        match first_divergence(&output.trace, &golden) {
            None => rows.push(format!("{}\tmatch", case.name)),
            Some(item) => {
                let row = format!(
                    "{}\tdiverge\ttick {}\n  actual   {}\n  golden   {}",
                    case.name, item.tick, item.actual, item.expected
                );
                rows.push(row.clone());
                failures.push(row);
            }
        }
    }
    let table = rows.join("\n");
    eprintln!("golden first-divergence\n{table}");
    assert!(
        failures.is_empty(),
        "golden mismatches:\n{}",
        failures.join("\n")
    );
}

fn check_nonboolean(actual: &str, golden: &str) -> Result<(), String> {
    let Some(found) = first_divergence(actual, golden) else {
        return Err("matched the aborting reference; the non-Boolean exception narrowed".into());
    };
    if found.tick != 1 {
        return Err(format!(
            "first divergence moved to tick {} (actual {} / golden {})",
            found.tick, found.actual, found.expected
        ));
    }
    if found.actual != NONBOOLEAN_TICK1 {
        return Err(format!(
            "tick 1 actual changed; exception widened:\n  actual   {}\n  pinned   {NONBOOLEAN_TICK1}",
            found.actual
        ));
    }
    if found.expected != NONBOOLEAN_ABORT {
        return Err(format!(
            "tick 1 golden abort changed:\n  golden   {}\n  pinned   {NONBOOLEAN_ABORT}",
            found.expected
        ));
    }
    let actual_tick0 = data_line(actual, 0).ok_or("engine emitted no tick 0")?;
    let golden_tick0 = data_line(golden, 0).ok_or("golden has no tick 0")?;
    if actual_tick0 != golden_tick0 {
        return Err(format!(
            "tick 0 diverged:\n  actual   {actual_tick0}\n  golden   {golden_tick0}"
        ));
    }
    Ok(())
}

fn data_line(text: &str, tick: u32) -> Option<&str> {
    let prefix = format!("T\t{tick}\t");
    text.lines().find(|line| line.starts_with(&prefix))
}
