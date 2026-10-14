//! Step budget, allocation budget, and steady-state allocation.

use shimeji_expr::{BudgetKind, EvalErrorKind, JavaRandom, Limits, StubHost, VarKind, VarMap};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

struct CountingAlloc;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static TEST_LOCK: Mutex<()> = Mutex::new(());

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn allocated() -> usize {
    ALLOCATED.load(Ordering::SeqCst)
}

#[test]
fn step_budget_stops_a_long_chain_quickly() {
    let _guard = TEST_LOCK.lock().expect("lock");
    // Each script is a shallow `v{n+1} + 1`. Walking the chain is deep;
    // the step budget must return before that walk reaches the stack limit.
    let count = 4_000;
    let mut names = Vec::with_capacity(count);
    let mut sources = Vec::with_capacity(count);
    for i in 0..count {
        names.push(format!("v{i}"));
        if i + 1 == count {
            sources.push("1".to_string());
        } else {
            sources.push(format!("v{} + 1", i + 1));
        }
    }
    let defs: Vec<(&str, VarKind, &str)> = names
        .iter()
        .zip(&sources)
        .map(|(name, source)| (name.as_str(), VarKind::Dollar, source.as_str()))
        .collect();
    let mut scope = VarMap::new();
    scope.define_all(&defs).expect("define");
    scope.init();
    let started = Instant::now();
    let error = scope
        .read(
            "v0",
            &StubHost::default(),
            &mut JavaRandom::new(1),
            Limits::new(40, 1024),
        )
        .expect_err("step budget");
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "eval ran for {:?}",
        started.elapsed()
    );
    assert_eq!(error.kind, EvalErrorKind::Budget);
    assert_eq!(error.budget_kind(), Some(BudgetKind::Steps));
}

#[test]
fn alloc_budget_stops_before_the_concat_string() {
    let _guard = TEST_LOCK.lock().expect("lock");
    let left = "a".repeat(2_000);
    let right = "b".repeat(2_000);
    let expr = format!("\"{left}\" + \"{right}\"");
    let mut scope = VarMap::new();
    let code = scope.compile(&expr).expect("compile");
    let before = allocated();
    let error = scope
        .eval(
            &code,
            &StubHost::default(),
            &mut JavaRandom::new(1),
            Limits::new(100, 8),
        )
        .expect_err("alloc budget");
    let delta = allocated() - before;
    assert_eq!(error.kind, EvalErrorKind::Budget);
    assert_eq!(error.budget_kind(), Some(BudgetKind::Alloc));
    assert!(
        delta < 256,
        "concat reserved 4000 bytes but allocated {delta} before failing"
    );
}

#[test]
fn steady_state_evaluation_allocates_nothing() {
    let _guard = TEST_LOCK.lock().expect("lock");
    let mut scope = VarMap::new();
    scope.bind_str("maxCount", "50");
    scope
        .define("r", VarKind::Dollar, "Math.random()")
        .expect("define");
    scope.init();
    let host = StubHost::nashorn();
    let mut rng = JavaRandom::new(42);
    let exprs = [
        "mascot.anchor.x+(mascot.environment.cursor.x-mascot.anchor.x)*Math.random()",
        "mascot.totalCount < maxCount",
        "!mascot.environment.floor.isOn(mascot.anchor)",
        "Math.random*100",
    ];
    let mut codes = Vec::new();
    for expr in exprs {
        codes.push(
            scope
                .compile(expr)
                .unwrap_or_else(|error| panic!("{expr}: {error}")),
        );
    }
    for code in &codes {
        scope
            .eval(code, &host, &mut rng, Limits::default())
            .expect("warmup");
    }
    scope
        .read("r", &host, &mut rng, Limits::default())
        .expect("warm variable");

    let before = allocated();
    for _ in 0..20 {
        for code in &codes {
            scope
                .eval(code, &host, &mut rng, Limits::default())
                .expect("steady");
        }
        scope
            .read("r", &host, &mut rng, Limits::default())
            .expect("cached variable");
    }
    assert_eq!(allocated() - before, 0);
}
