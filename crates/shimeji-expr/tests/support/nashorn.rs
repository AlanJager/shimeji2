//! Replay `tests/vectors/nashorn.json` against the evaluator.

use shimeji_expr::{JavaRandom, Limits, StubHost, Value, VarMap};
use std::path::PathBuf;

pub fn assert_vectors_match() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/nashorn.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("read {}: {error}", path.display());
    });
    let root: serde_json::Value = serde_json::from_str(&text).expect("nashorn.json");
    let cases = root["cases"].as_array().expect("cases");
    let mut failures = Vec::new();
    for case in cases {
        if let Some(failure) = check_case(case) {
            failures.push(failure);
        }
    }
    assert!(
        failures.is_empty(),
        "{} Nashorn mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn check_case(case: &serde_json::Value) -> Option<String> {
    let id = case["id"].as_str().unwrap_or("?");
    let expr = case["expr"].as_str().expect("expr");
    let mut scope = nashorn_scope();
    let host = StubHost::nashorn();
    let mut rng = JavaRandom::new(42);
    let code = match scope.compile(expr) {
        Ok(code) => code,
        Err(error) => return Some(format!("{id}: compile {error} :: {expr}")),
    };
    let result = scope.eval(&code, &host, &mut rng, Limits::default());
    let draws = case["draws"].as_u64().expect("draws");
    if let Some(failure) = draws_failure(id, draws, &rng) {
        return Some(failure);
    }
    if case["ok"].as_bool() != Some(true) {
        return match result {
            Err(_) => None,
            Ok(value) => Some(format!(
                "{id}: expected an error, got {}",
                describe(&scope, value)
            )),
        };
    }
    let value = match result {
        Ok(value) => value,
        Err(error) => return Some(format!("{id}: {error} :: {expr}")),
    };
    let expected_type = case["type"].as_str().unwrap_or("");
    if value.type_name() != expected_type {
        return Some(format!(
            "{id}: type {} != {expected_type} ({})",
            value.type_name(),
            describe(&scope, value)
        ));
    }
    match expected_type {
        "number" => {
            let got = value.as_number().unwrap().to_bits();
            let expected =
                u64::from_str_radix(case["bits"].as_str().unwrap_or(""), 16).expect("bits");
            // Script code cannot tell NaNs apart, and the CPU picks the bits of
            // a new one: x86-64 sets the sign on 0/0, arm64 does not.
            let both_nan = f64::from_bits(got).is_nan() && f64::from_bits(expected).is_nan();
            if got != expected && !both_nan {
                return Some(format!(
                    "{id}: bits {got:016x} != {expected:016x} :: {expr}"
                ));
            }
        }
        "boolean" => {
            let expected = case["value"].as_bool().expect("bool");
            if value.as_bool() != Some(expected) {
                return Some(format!("{id}: bool {:?} != {expected}", value.as_bool()));
            }
        }
        "string" => {
            let expected = case["value"].as_str().unwrap_or("");
            let got = scope.text(value).unwrap_or("");
            if got != expected {
                return Some(format!("{id}: string {got:?} != {expected:?}"));
            }
        }
        other => return Some(format!("{id}: unhandled type {other}")),
    }
    None
}

fn draws_failure(id: &str, draws: u64, rng: &JavaRandom) -> Option<String> {
    let mut probe = JavaRandom::new(42);
    let mut expected = 0.0;
    for _ in 0..=draws {
        expected = probe.next_double();
    }
    let got = rng.clone().next_double();
    if got.to_bits() != expected.to_bits() {
        return Some(format!(
            "{id}: after {draws} draws, next random {:016x} != {:016x}",
            got.to_bits(),
            expected.to_bits()
        ));
    }
    None
}

fn nashorn_scope() -> VarMap {
    let mut scope = VarMap::new();
    scope.bind_str("maxCount", "50");
    scope.bind_number("目的地Y", 150.0);
    scope.bind_number("ずれ", 10.0);
    scope.bind_number("footX", 90.0);
    scope.bind_number("FootX", 90.0);
    scope.bind_number("TargetY", 150.0);
    scope.bind_bool("a", true);
    scope
}

fn describe(scope: &VarMap, value: Value) -> String {
    match value {
        Value::Number(n) => format!("number {n} bits={:016x}", n.to_bits()),
        Value::Bool(b) => format!("boolean {b}"),
        Value::Str(_) => format!("string {:?}", scope.text(value).unwrap_or("")),
        other => other.type_name().to_string(),
    }
}
