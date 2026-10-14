//! JDK-captured `java.util.Random` sequences must match bit-for-bit.

use shimeji_expr::JavaRandom;
use std::fs;
use std::path::PathBuf;

fn vectors() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/java_random.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).expect("java_random.json")
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn each_seed(doc: &serde_json::Value) -> impl Iterator<Item = (i64, &serde_json::Value)> {
    doc["seeds"].as_array().expect("seeds").iter().map(|block| {
        let seed = block["seed"].as_i64().expect("seed");
        (seed, block)
    })
}

#[test]
fn next_double_matches_jdk() {
    let doc = vectors();
    for (seed, block) in each_seed(&doc) {
        let mut rng = JavaRandom::new(seed);
        for (index, entry) in block["nextDouble"]
            .as_array()
            .expect("nextDouble")
            .iter()
            .enumerate()
        {
            let got = rng.next_double();
            let expected = entry["bits"].as_str().expect("bits");
            assert_eq!(bits(got), expected, "seed {seed} nextDouble[{index}]");
        }
    }
}

#[test]
fn next_int_100_matches_jdk() {
    let doc = vectors();
    for (seed, block) in each_seed(&doc) {
        let mut rng = JavaRandom::new(seed);
        for (index, entry) in block["nextInt_100"]
            .as_array()
            .expect("nextInt_100")
            .iter()
            .enumerate()
        {
            let got = rng.next_int_bound(100);
            let expected = entry.as_i64().expect("int") as i32;
            assert_eq!(got, expected, "seed {seed} nextInt(100)[{index}]");
        }
    }
}

#[test]
fn next_int_matches_jdk() {
    let doc = vectors();
    for (seed, block) in each_seed(&doc) {
        let mut rng = JavaRandom::new(seed);
        for (index, entry) in block["nextInt"]
            .as_array()
            .expect("nextInt")
            .iter()
            .enumerate()
        {
            let got = rng.next_int();
            let expected = entry.as_i64().expect("int") as i32;
            assert_eq!(got, expected, "seed {seed} nextInt()[{index}]");
        }
    }
}

#[test]
fn next_int_power_of_two_matches_jdk() {
    let doc = vectors();
    for (seed, block) in each_seed(&doc) {
        let mut rng = JavaRandom::new(seed);
        for (index, entry) in block["nextInt_8"]
            .as_array()
            .expect("nextInt_8")
            .iter()
            .enumerate()
        {
            let got = rng.next_int_bound(8);
            let expected = entry.as_i64().expect("int") as i32;
            assert_eq!(got, expected, "seed {seed} nextInt(8)[{index}]");
        }
    }
}

#[test]
fn next_int_rejection_matches_jdk() {
    let doc = vectors();
    for (seed, block) in each_seed(&doc) {
        let mut rng = JavaRandom::new(seed);
        for (index, entry) in block["nextInt_3"]
            .as_array()
            .expect("nextInt_3")
            .iter()
            .enumerate()
        {
            let got = rng.next_int_bound(3);
            let expected = entry.as_i64().expect("int") as i32;
            assert_eq!(got, expected, "seed {seed} nextInt(3)[{index}]");
        }
    }
}

#[test]
fn next_boolean_matches_jdk() {
    let doc = vectors();
    for (seed, block) in each_seed(&doc) {
        let mut rng = JavaRandom::new(seed);
        for (index, entry) in block["nextBoolean"]
            .as_array()
            .expect("nextBoolean")
            .iter()
            .enumerate()
        {
            let got = rng.next_boolean();
            let expected = entry.as_bool().expect("bool");
            assert_eq!(got, expected, "seed {seed} nextBoolean[{index}]");
        }
    }
}

#[test]
fn set_seed_replays_the_same_stream() {
    let mut rng = JavaRandom::new(42);
    let first = rng.next_double();
    rng.set_seed(42);
    assert_eq!(rng.next_double().to_bits(), first.to_bits());
}
