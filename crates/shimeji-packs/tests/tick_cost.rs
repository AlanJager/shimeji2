//! `engine.tick` cost for a shime population, and the 100_000-tick smoke run.
//!
//! The numbers are printed. The plan's p99 targets (100 µs at 10, 500 µs at 50)
//! are compared in the unit report; this test only fails if a tick blows up.

use std::path::PathBuf;
use std::time::Instant;

use shimeji_core::Engine;
use shimeji_core::env::{Point, ScreenSample, Snapshot};
use shimeji_core::model::Character;
use shimeji_packs::load_dir;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn shime() -> Character {
    let loaded = load_dir(root().join("assets/packs/shime")).expect("shime");
    loaded
        .pack
        .character("shime")
        .expect("shime character")
        .clone()
}

fn world() -> Snapshot {
    Snapshot {
        screens: vec![ScreenSample {
            id: "main".to_string(),
            x: -4500,
            y: 0,
            width: 6000,
            height: 800,
            work: None,
        }],
        cursor: Point::new(200, 200),
        active_window: None,
        multiscreen: true,
    }
}

fn population(count: usize) -> (Engine, Snapshot) {
    let mut engine = Engine::new(&shime(), 1);
    let snapshot = world();
    engine.apply(&snapshot);
    for _ in 0..count {
        engine.spawn();
    }
    (engine, snapshot)
}

fn percentile(samples: &[u128], pct: usize) -> u128 {
    let index = (samples.len() * pct / 100).min(samples.len() - 1);
    samples[index]
}

#[test]
fn tick_p50_and_p99_for_10_and_50_shime() {
    for count in [10, 50] {
        let (mut engine, snapshot) = population(count);
        for _ in 0..1_000 {
            engine.tick(&snapshot);
        }
        let mut samples = Vec::with_capacity(500);
        for _ in 0..500 {
            let started = Instant::now();
            engine.tick(&snapshot);
            samples.push(started.elapsed().as_nanos());
        }
        samples.sort_unstable();
        let p50 = percentile(&samples, 50);
        let p99 = percentile(&samples, 99);
        eprintln!(
            "engine.tick n={count} p50={}µs p99={}µs",
            p50 / 1_000,
            p99 / 1_000
        );
        assert!(
            p99 < 50_000_000,
            "n={count} p99 {p99} ns is far past the plan"
        );
    }
}

#[test]
fn fifty_shime_mascots_survive_one_hundred_thousand_ticks() {
    let (mut engine, snapshot) = population(50);
    let started = Instant::now();
    for _ in 0..100_000 {
        engine.tick(&snapshot);
    }
    eprintln!(
        "smoke 50x100000 finished in {:?} population {}",
        started.elapsed(),
        engine.population()
    );
    assert!(engine.population() <= 50);
    assert!(engine.population() > 0);
}
