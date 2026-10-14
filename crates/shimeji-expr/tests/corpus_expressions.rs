//! Parses and evaluates `corpus/expressions.json` when present.
//! Ignored in CI: the file is gitignored.

use shimeji_expr::{JavaRandom, Limits, MascotFields, StubHost, VarMap, parse_script};
use std::fs;
use std::path::PathBuf;

fn corpus_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/expressions.json")
}

fn load_exprs() -> Option<Vec<String>> {
    let path = corpus_path();
    if !path.is_file() {
        eprintln!("skip: {} is not present", path.display());
        return None;
    }
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let items: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("corpus/expressions.json is a JSON array");
    Some(
        items
            .iter()
            .map(|item| {
                item.get("expr")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect(),
    )
}

#[ignore]
#[test]
fn corpus_expressions_parse_when_present() {
    let Some(exprs) = load_exprs() else {
        return;
    };
    let mut failures = Vec::new();
    for expr in &exprs {
        if expr.is_empty() {
            failures.push("object missing string field \"expr\"".to_string());
            continue;
        }
        if let Err(error) = parse_script(expr) {
            failures.push(format!("{error} :: {expr}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{}/{} out of subset:\n{}",
        failures.len(),
        exprs.len(),
        failures.join("\n")
    );
}

#[ignore]
#[test]
fn corpus_expressions_evaluate_when_present() {
    let Some(exprs) = load_exprs() else {
        return;
    };
    let mut scope = VarMap::new();
    scope.bind_number("Gap", 12.0);
    scope.bind_number("FootX", 90.0);
    scope.bind_number("footX", 90.0);
    scope.bind_number("TargetY", 150.0);
    scope.bind_number("目的地Y", 150.0);
    scope.bind_number("ずれ", 10.0);
    scope.bind_str("maxCount", "50");
    let mut compiled = Vec::new();
    for expr in &exprs {
        match scope.compile(expr) {
            Ok(code) => compiled.push((expr.clone(), code)),
            Err(error) => panic!("compile {error} :: {expr}"),
        }
    }
    let mut failures = Vec::new();
    for seed in 0..1_000 {
        let host = random_host(seed);
        let mut rng = JavaRandom::new(seed);
        for (expr, code) in &compiled {
            if let Err(error) = scope.eval(code, &host, &mut rng, Limits::default()) {
                failures.push(format!("seed {seed}: {error} :: {expr}"));
                if failures.len() >= 8 {
                    break;
                }
            }
        }
        if failures.len() >= 8 {
            break;
        }
    }
    assert!(
        failures.is_empty(),
        "{} corpus eval failures (showing up to 8):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn random_host(seed: i64) -> StubHost {
    let mut rng = JavaRandom::new(seed);
    StubHost {
        mascot: random_mascot(&mut rng),
        target: random_mascot(&mut rng),
    }
}

fn random_mascot(rng: &mut JavaRandom) -> MascotFields {
    MascotFields {
        anchor_x: spread(rng),
        anchor_y: spread(rng),
        look_right: rng.next_boolean(),
        total_count: rng.next_double() * 20.0,
        cursor_x: spread(rng),
        cursor_y: spread(rng),
        cursor_dx: spread(rng),
        cursor_dy: spread(rng),
        screen: random_area(rng),
        work_area: random_area(rng),
        active_ie: random_area(rng),
        floor: rng.next_boolean(),
        ceiling: rng.next_boolean(),
        wall: rng.next_boolean(),
    }
}

fn random_area(rng: &mut JavaRandom) -> shimeji_expr::AreaFields {
    let left = spread(rng);
    let top = spread(rng);
    shimeji_expr::AreaFields {
        left,
        top,
        right: left + rng.next_double() * 800.0,
        bottom: top + rng.next_double() * 600.0,
        visible: rng.next_boolean(),
        on_left: rng.next_boolean(),
        on_right: rng.next_boolean(),
        on_top: rng.next_boolean(),
        on_bottom: rng.next_boolean(),
    }
}

fn spread(rng: &mut JavaRandom) -> f64 {
    (rng.next_double() - 0.5) * 2_000.0
}
