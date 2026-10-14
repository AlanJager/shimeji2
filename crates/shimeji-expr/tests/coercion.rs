//! JavaScript coercion and the Java `int` sink. Every Nashorn vector is checked here too.

#[path = "support/nashorn.rs"]
mod nashorn;

use shimeji_expr::{JavaRandom, Limits, StubHost, VarMap, to_i32};

#[test]
fn nashorn_vectors_match_type_and_value() {
    nashorn::assert_vectors_match();
}

#[test]
fn string_plus_number_concatenates() {
    assert_eq!(eval_str("\"5\" + 1"), "51");
    assert_eq!(eval_str("1 + \"5\""), "15");
    // `"5" + 1` is already a string, so a further `+ 1` stays a string.
    // The number stays numeric only after unary plus.
    assert_eq!(eval_str("\"5\" + 1 + 1"), "511");
    assert_eq!(eval_num("+\"5\" + 1").to_bits(), 6.0f64.to_bits());
}

#[test]
fn relational_coerces_numeric_string() {
    let mut scope = VarMap::new();
    scope.bind_str("maxCount", "50");
    let host = StubHost::nashorn();
    assert!(!eval_in(&mut scope, &host, "maxCount < 50"));
    assert!(eval_in(&mut scope, &host, "maxCount < 51"));
    assert!(eval_in(&mut scope, &host, "mascot.totalCount < maxCount"));
    let mut rng = JavaRandom::new(42);
    let bound = scope
        .read("maxCount", &host, &mut rng, Limits::default())
        .expect("maxCount");
    assert_eq!(scope.text(bound), Some("50"));
}

#[test]
fn division_by_zero_is_infinity_or_nan() {
    assert_eq!(eval_num("1/0").to_bits(), f64::INFINITY.to_bits());
    assert_eq!(eval_num("-1/0").to_bits(), f64::NEG_INFINITY.to_bits());
    assert!(eval_num("0/0").is_nan());
}

#[test]
fn nan_comparisons_are_false() {
    for expr in [
        "NaN < 1",
        "NaN > 1",
        "NaN <= NaN",
        "NaN >= 0",
        "NaN == NaN",
        "NaN === NaN",
    ] {
        assert!(!eval_bool(expr), "{expr}");
    }
    assert!(eval_bool("NaN != NaN"));
    assert!(eval_bool("NaN !== NaN"));
}

#[test]
fn bare_math_random_is_nan_and_java_int_is_zero() {
    let mut scope = VarMap::new();
    let code = scope.compile("Math.random*100").expect("compile");
    let mut rng = JavaRandom::new(42);
    let before = rng.clone().next_double();
    let value = scope
        .eval(&code, &StubHost::nashorn(), &mut rng, Limits::default())
        .expect("eval");
    assert!(value.as_number().unwrap().is_nan());
    assert_eq!(scope.to_java_int(value), 0);
    assert_eq!(rng.clone().next_double().to_bits(), before.to_bits());
}

#[test]
fn java_int_sink_on_literals() {
    assert_eq!(to_i32(1.9), 1);
    assert_eq!(to_i32(-1.9), -1);
    assert_eq!(to_i32(-0.9), 0);
    assert_eq!(to_i32(f64::NAN), 0);
    assert_eq!(to_i32(f64::INFINITY), i32::MAX);
    assert_eq!(to_i32(f64::NEG_INFINITY), i32::MIN);
    assert_eq!(to_i32(1e20), i32::MAX);
    assert_eq!(to_i32(-1e20), i32::MIN);
    let mut scope = VarMap::new();
    let code = scope.compile("1.9").expect("compile");
    let value = scope
        .eval(
            &code,
            &StubHost::default(),
            &mut JavaRandom::new(1),
            Limits::default(),
        )
        .expect("eval");
    assert_eq!(scope.to_java_int(value), 1);
}

fn eval_num(expr: &str) -> f64 {
    let mut scope = VarMap::new();
    let code = scope
        .compile(expr)
        .unwrap_or_else(|error| panic!("{expr}: {error}"));
    let value = scope
        .eval(
            &code,
            &StubHost::nashorn(),
            &mut JavaRandom::new(42),
            Limits::default(),
        )
        .unwrap_or_else(|error| panic!("{expr}: {error}"));
    value
        .as_number()
        .unwrap_or_else(|| panic!("{expr}: not a number"))
}

fn eval_bool(expr: &str) -> bool {
    let mut scope = VarMap::new();
    let code = scope
        .compile(expr)
        .unwrap_or_else(|error| panic!("{expr}: {error}"));
    let value = scope
        .eval(
            &code,
            &StubHost::nashorn(),
            &mut JavaRandom::new(42),
            Limits::default(),
        )
        .unwrap_or_else(|error| panic!("{expr}: {error}"));
    value
        .as_bool()
        .unwrap_or_else(|| panic!("{expr}: not a boolean"))
}

fn eval_str(expr: &str) -> String {
    let mut scope = VarMap::new();
    let code = scope
        .compile(expr)
        .unwrap_or_else(|error| panic!("{expr}: {error}"));
    let value = scope
        .eval(
            &code,
            &StubHost::nashorn(),
            &mut JavaRandom::new(42),
            Limits::default(),
        )
        .unwrap_or_else(|error| panic!("{expr}: {error}"));
    scope.text(value).expect("string").to_string()
}

fn eval_in(scope: &mut VarMap, host: &StubHost, expr: &str) -> bool {
    let code = scope
        .compile(expr)
        .unwrap_or_else(|error| panic!("{expr}: {error}"));
    let value = scope
        .eval(&code, host, &mut JavaRandom::new(42), Limits::default())
        .unwrap_or_else(|error| panic!("{expr}: {error}"));
    value
        .as_bool()
        .unwrap_or_else(|| panic!("{expr}: not a boolean"))
}
