//! Bundled shime → native → classic, compared with the U7 goldens.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use shimeji_core::testing::{Scenario, first_divergence, run_output};
use shimeji_packs::model::{Action, ActionNode, Character};
use shimeji_packs::{Severity, export_classic, load_dir, write_standard_pack};

const CASES: &[&str] = &[
    "shime-floor-only",
    "shime-drag-throw",
    "shime-breed-limit",
    "shime-moving-window",
    "shime-offscreen-teleport",
    "shime-two-screens",
    "shime-window-drag-up",
];

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("shimeji-u13-round-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn anchors(character: &Character, source: &str) -> BTreeSet<(i32, i32)> {
    let mut found = BTreeSet::new();
    fn walk(action: &Action, source: &str, found: &mut BTreeSet<(i32, i32)>) {
        for animation in &action.animations {
            for pose in &animation.poses {
                if pose.image_source.as_deref() == Some(source) {
                    found.insert((pose.anchor_x, pose.anchor_y));
                }
            }
        }
        for child in &action.children {
            if let ActionNode::Action(child) = child {
                walk(child, source, found);
            }
        }
    }
    for action in character.actions.values() {
        walk(action, source, &mut found);
    }
    found
}

#[test]
fn shime_native_then_classic_matches_the_u7_goldens() {
    let root = repo();
    let work = scratch();
    let native = work.join("native");
    let classic = work.join("shime");
    write_standard_pack(&root.join("assets/packs/shime"), &native).unwrap();
    export_classic(&native, &classic).unwrap();

    let actions = fs::read(classic.join("conf/動作.xml")).unwrap();
    let behaviors = fs::read(classic.join("conf/行動.xml")).unwrap();
    assert_eq!(
        actions,
        fs::read(root.join("assets/packs/shime/conf/動作.xml")).unwrap()
    );
    assert_eq!(
        behaviors,
        fs::read(root.join("assets/packs/shime/conf/行動.xml")).unwrap()
    );

    let loaded = load_dir(&classic).unwrap();
    let errors: Vec<_> = loaded
        .diagnostics
        .iter()
        .filter(|diag| diag.severity == Severity::Error)
        .map(|diag| diag.message.clone())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let character = loaded.pack.character("shime").expect("image set shime");
    assert_eq!(
        anchors(character, "/shime9.png"),
        BTreeSet::from([(64, 128), (32, 128)])
    );
    assert_eq!(
        anchors(character, "/shime19.png"),
        BTreeSet::from([(64, 128), (64, 104)])
    );

    let mut failures = Vec::new();
    for name in CASES {
        let text = fs::read_to_string(root.join(format!("tests/scenarios/{name}.json"))).unwrap();
        let scenario = Scenario::parse(&text).unwrap();
        let output = run_output(character, &scenario);
        let trips = output
            .diagnostics
            .iter()
            .filter(|diag| diag.message.contains("evaluation budget exhausted"))
            .count();
        if trips != 0 {
            failures.push(format!("{name}: evaluation budget tripped {trips} time(s)"));
        }
        let golden =
            fs::read_to_string(root.join(format!("tests/golden/shime/{name}.trace"))).unwrap();
        if let Some(item) = first_divergence(&output.trace, &golden) {
            failures.push(format!(
                "{name} diverged at tick {}\n  actual   {}\n  golden   {}",
                item.tick, item.actual, item.expected
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let _ = fs::remove_dir_all(&work);
}
