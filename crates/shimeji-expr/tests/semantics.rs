//! Evaluation semantics from the Unit 4 plan, including every Nashorn vector.

#[path = "support/nashorn.rs"]
mod nashorn;

use shimeji_expr::{
    CallSite, EvalErrorKind, JavaRandom, Limits, SiteEffect, StubHost, VarKind, VarMap,
    condition_bool, decide, decide_parse, parse,
};

#[test]
fn nashorn_vectors_match_type_and_value() {
    nashorn::assert_vectors_match();
}

#[test]
fn dollar_math_random_draws_on_first_read_at_tick_3() {
    let mut scope = VarMap::new();
    scope
        .define("r", VarKind::Dollar, "Math.random()")
        .expect("define");
    let mut rng = JavaRandom::new(42);
    let host = StubHost::nashorn();
    let fingerprint = rng.clone().next_double().to_bits();
    scope.init();
    assert_eq!(
        rng.clone().next_double().to_bits(),
        fingerprint,
        "init must not evaluate a dollar variable"
    );
    scope.next();
    scope.next();
    assert_eq!(
        rng.clone().next_double().to_bits(),
        fingerprint,
        "unread dollar variable survives next()"
    );

    let mut probe = JavaRandom::new(42);
    let first = probe.next_double();
    let second = probe.next_double();
    let value = scope
        .read("r", &host, &mut rng, Limits::default())
        .expect("tick 3 read");
    assert_eq!(value.as_number().unwrap().to_bits(), first.to_bits());
    assert_eq!(rng.clone().next_double().to_bits(), second.to_bits());

    let again = scope
        .read("r", &host, &mut rng, Limits::default())
        .expect("cached read");
    assert_eq!(again.as_number().unwrap().to_bits(), first.to_bits());
    assert_eq!(
        rng.clone().next_double().to_bits(),
        second.to_bits(),
        "second read stays cached"
    );
}

#[test]
fn sibling_reference_draws_b_before_a() {
    let mut scope = VarMap::new();
    scope
        .define_all(&[
            ("b", VarKind::Dollar, "Math.random()"),
            ("a", VarKind::Dollar, "b + Math.random()"),
        ])
        .expect("define");
    scope.init();
    let mut rng = JavaRandom::new(42);
    let host = StubHost::nashorn();

    let mut probe = JavaRandom::new(42);
    let b_draw = probe.next_double();
    let a_draw = probe.next_double();
    let third = probe.next_double();

    let a = scope
        .read("a", &host, &mut rng, Limits::default())
        .expect("read a");
    assert_eq!(
        a.as_number().unwrap().to_bits(),
        (b_draw + a_draw).to_bits()
    );
    let b = scope
        .read("b", &host, &mut rng, Limits::default())
        .expect("read b");
    assert_eq!(b.as_number().unwrap().to_bits(), b_draw.to_bits());
    assert_eq!(
        rng.clone().next_double().to_bits(),
        third.to_bits(),
        "b was drawn while evaluating a, and is cached"
    );
}

#[test]
fn hash_null_and_undefined_stay_cached_with_false_and_zero() {
    let mut scope = VarMap::new();
    scope
        .define_all(&[
            ("f", VarKind::Hash, "Math.random() < 0"),
            ("z", VarKind::Hash, "Math.random() * 0"),
            ("n", VarKind::Hash, "(Math.random() < 2) ? null : 1"),
            ("u", VarKind::Hash, "(Math.random() < 2) ? Math.missing : 1"),
        ])
        .expect("define");
    scope.init();
    let mut rng = JavaRandom::new(42);
    let mut expected = JavaRandom::new(42);
    let host = StubHost::nashorn();

    let f = scope
        .read("f", &host, &mut rng, Limits::default())
        .expect("false");
    assert_eq!(f.as_bool(), Some(false));
    expected.next_double();
    assert_same_rng(&rng, &expected);

    let f2 = scope
        .read("f", &host, &mut rng, Limits::default())
        .expect("cached false");
    assert_eq!(f2.as_bool(), Some(false));
    assert_same_rng(&rng, &expected);

    let z = scope
        .read("z", &host, &mut rng, Limits::default())
        .expect("zero");
    assert_eq!(z.as_number().unwrap().to_bits(), 0.0f64.to_bits());
    expected.next_double();
    assert_same_rng(&rng, &expected);

    let z2 = scope
        .read("z", &host, &mut rng, Limits::default())
        .expect("cached zero");
    assert_eq!(z2.as_number().unwrap().to_bits(), 0.0f64.to_bits());
    assert_same_rng(&rng, &expected);

    let n = scope
        .read("n", &host, &mut rng, Limits::default())
        .expect("null");
    assert!(n.is_null());
    expected.next_double();
    assert_same_rng(&rng, &expected);

    let n2 = scope
        .read("n", &host, &mut rng, Limits::default())
        .expect("cached null");
    assert!(n2.is_null());
    assert_same_rng(&rng, &expected);

    let u = scope
        .read("u", &host, &mut rng, Limits::default())
        .expect("undefined");
    assert!(matches!(u, shimeji_expr::Value::Undefined));
    expected.next_double();
    assert_same_rng(&rng, &expected);
    let u2 = scope
        .read("u", &host, &mut rng, Limits::default())
        .expect("cached undefined");
    assert!(matches!(u2, shimeji_expr::Value::Undefined));
    assert_same_rng(&rng, &expected);

    scope.next();
    let f3 = scope
        .read("f", &host, &mut rng, Limits::default())
        .expect("false after next");
    assert_eq!(f3.as_bool(), Some(false));
    expected.next_double();
    assert_same_rng(&rng, &expected);
    let _ = scope
        .read("z", &host, &mut rng, Limits::default())
        .expect("zero after next");
    expected.next_double();
    assert_same_rng(&rng, &expected);
}

#[test]
fn hash_null_math_random_draws_once_between_next_calls() {
    let mut scope = VarMap::new();
    scope
        .define("n", VarKind::Hash, "(Math.random() < 2) ? null : 1")
        .expect("define");
    scope.init();
    let mut rng = JavaRandom::new(42);
    let mut expected = JavaRandom::new(42);
    let host = StubHost::nashorn();

    scope.next();
    let first = scope
        .read("n", &host, &mut rng, Limits::default())
        .expect("null");
    assert!(first.is_null());
    expected.next_double();
    let second = scope
        .read("n", &host, &mut rng, Limits::default())
        .expect("cached null");
    assert!(second.is_null());
    assert_same_rng(&rng, &expected);
    scope.next();
}

#[test]
fn dollar_false_stays_cached_across_next() {
    let mut scope = VarMap::new();
    scope
        .define("f", VarKind::Dollar, "Math.random() < 0")
        .expect("define");
    scope.init();
    let mut rng = JavaRandom::new(42);
    let mut expected = JavaRandom::new(42);
    let host = StubHost::nashorn();
    let first = scope
        .read("f", &host, &mut rng, Limits::default())
        .expect("read");
    assert_eq!(first.as_bool(), Some(false));
    expected.next_double();
    scope.next();
    let second = scope
        .read("f", &host, &mut rng, Limits::default())
        .expect("after next");
    assert_eq!(second.as_bool(), Some(false));
    assert_same_rng(&rng, &expected);
    scope.init();
    let _ = scope
        .read("f", &host, &mut rng, Limits::default())
        .expect("after init");
    expected.next_double();
    assert_same_rng(&rng, &expected);
}

#[test]
fn variable_cycle_is_an_error() {
    let mut scope = VarMap::new();
    scope
        .define_all(&[("a", VarKind::Dollar, "b"), ("b", VarKind::Dollar, "a")])
        .expect("define");
    scope.init();
    let mut rng = JavaRandom::new(42);
    let error = scope
        .read("a", &StubHost::nashorn(), &mut rng, Limits::default())
        .expect_err("cycle");
    assert_eq!(error.kind, EvalErrorKind::Cycle);
    assert_eq!(error.span.start, 0);
    assert_eq!(error.span.end, 1);
    assert!(error.message.contains('a'));
}

#[test]
fn long_variable_cycle_does_not_overflow() {
    let mut scope = VarMap::new();
    let mut defs = Vec::new();
    let sources: Vec<String> = (0..40).map(|i| format!("v{}", (i + 1) % 40)).collect();
    for (i, source) in sources.iter().enumerate() {
        defs.push((format!("v{i}"), source.as_str()));
    }
    let borrowed: Vec<(&str, VarKind, &str)> = defs
        .iter()
        .map(|(name, source)| (name.as_str(), VarKind::Dollar, *source))
        .collect();
    scope.define_all(&borrowed).expect("define");
    scope.init();
    let mut rng = JavaRandom::new(42);
    let error = scope
        .read("v0", &StubHost::nashorn(), &mut rng, Limits::default())
        .expect_err("cycle");
    assert_eq!(error.kind, EvalErrorKind::Cycle);
}

#[test]
fn short_circuit_or_true_draws_nothing() {
    let mut scope = VarMap::new();
    scope.bind_bool("a", true);
    let code = scope.compile("a || Math.random() < 0.5").expect("compile");
    let mut rng = JavaRandom::new(42);
    let before = rng.clone().next_double();
    let value = scope
        .eval(&code, &StubHost::nashorn(), &mut rng, Limits::default())
        .expect("eval");
    assert_eq!(value.as_bool(), Some(true));
    assert_eq!(rng.clone().next_double().to_bits(), before.to_bits());
}

#[test]
fn unknown_identifier_has_a_span_and_behavior_condition_is_false() {
    let mut scope = VarMap::new();
    let code = scope.compile("1 + notAThing").expect("compile");
    let mut rng = JavaRandom::new(42);
    let error = scope
        .eval(&code, &StubHost::nashorn(), &mut rng, Limits::default())
        .expect_err("reference");
    assert_eq!(error.kind, EvalErrorKind::Reference);
    assert_eq!(error.span.start, 4);
    assert_eq!(error.span.end, 13);
    let behavior = decide(CallSite::BehaviorCondition, &error);
    assert_eq!(behavior.effect, SiteEffect::False);
    assert_eq!(behavior.diagnostic.span, error.span);
    let action = decide(CallSite::ActionCondition, &error);
    assert_eq!(action.effect, SiteEffect::Dispose);
}

#[test]
fn syntax_error_compile_diagnostic_has_a_span() {
    let error = parse("foo = 1").expect_err("assignment");
    assert_eq!(error.span.start, 4);
    assert_eq!(error.span.end, 5);
    let behavior = decide_parse(CallSite::BehaviorCondition, &error);
    assert_eq!(behavior.effect, SiteEffect::False);
    assert_eq!(behavior.diagnostic.span.start, 4);
    assert_eq!(behavior.diagnostic.span.end, 5);
    assert!(!behavior.diagnostic.message.is_empty());
    let action = decide_parse(CallSite::ActionParameter, &error);
    assert_eq!(action.effect, SiteEffect::Dispose);
    assert_eq!(action.diagnostic.span, error.span);
}

#[test]
fn non_boolean_condition_is_false_for_behavior_and_action() {
    let mut scope = VarMap::new();
    let code = scope.compile("1").expect("compile");
    let mut rng = JavaRandom::new(42);
    let value = scope
        .eval(&code, &StubHost::nashorn(), &mut rng, Limits::default())
        .expect("eval");
    let error = condition_bool(value, code.span()).expect_err("not a boolean");
    assert_eq!(error.kind, EvalErrorKind::NonBoolean);
    assert_eq!(
        decide(CallSite::BehaviorCondition, &error).effect,
        SiteEffect::False
    );
    assert_eq!(
        decide(CallSite::ActionCondition, &error).effect,
        SiteEffect::False
    );
}

fn assert_same_rng(rng: &JavaRandom, expected: &JavaRandom) {
    assert_eq!(
        rng.clone().next_double().to_bits(),
        expected.clone().next_double().to_bits()
    );
}
