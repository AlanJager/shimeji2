//! Ignored corpus sweep. Run with `cargo test -p shimeji-packs --test corpus_load -- --ignored`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use shimeji_packs::load_dir;

#[test]
#[ignore]
fn load_corpus_packs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/packs");
    let mut paths: Vec<_> = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("read {}: {error}", root.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    paths.sort();

    let mut loaded = 0;
    let mut degraded = 0;
    let mut failed = 0;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();

    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy();
        let outcome = match load_dir(path) {
            Ok(outcome) => outcome,
            Err(error) => {
                failed += 1;
                println!("{name}\tfailed\t0\tio {error}");
                continue;
            }
        };
        let status = if outcome.pack.characters.is_empty() {
            failed += 1;
            "failed"
        } else if outcome
            .diagnostics
            .iter()
            .any(|diag| diag.is_stay_fallback())
        {
            degraded += 1;
            "degraded"
        } else {
            loaded += 1;
            "loaded"
        };
        let mut local: BTreeMap<String, usize> = BTreeMap::new();
        for diag in &outcome.diagnostics {
            let kind = format!("{:?}", diag.kind);
            *local.entry(kind.clone()).or_default() += 1;
            *kinds.entry(kind).or_default() += 1;
        }
        let summary = local
            .iter()
            .map(|(kind, count)| format!("{kind}:{count}"))
            .collect::<Vec<_>>()
            .join(",");
        println!("{name}\t{status}\t{}\t{summary}", outcome.diagnostics.len());
        if status == "failed" {
            for diag in &outcome.diagnostics {
                println!(
                    "  FAIL {name} {:?} {}:{} {}",
                    diag.severity,
                    diag.file,
                    diag.line.map(|line| line.to_string()).unwrap_or_default(),
                    diag.message
                );
            }
        }
        if outcome
            .diagnostics
            .iter()
            .any(|diag| diag.is_stay_fallback())
        {
            for diag in outcome
                .diagnostics
                .iter()
                .filter(|diag| diag.is_stay_fallback())
            {
                println!("  STAY {name} {}: {}", diag.file, diag.message);
            }
        }
    }

    let total = loaded + degraded + failed;
    println!("TOTAL loaded={loaded} degraded={degraded} failed={failed} total={total}");
    for (kind, count) in &kinds {
        println!("KIND {kind} {count}");
    }
    assert!(total >= 50, "expected the 50-pack corpus, found {total}");
    let ok = loaded + degraded;
    assert!(
        ok * 100 / total >= 95,
        "loaded+degraded {ok}/{total} is under 95%"
    );
}

/// Same corpus, through `import_pack` on each `archive.zip`.
/// Every archive is named `archive.zip`, so each pack gets its own packs root.
#[test]
#[ignore]
fn import_corpus_packs() {
    use shimeji_packs::{CollisionPolicy, import_pack};

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/packs");
    let mut paths: Vec<_> = fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("read {}: {error}", root.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    paths.sort();

    let work = std::env::temp_dir().join(format!("shimeji-u5b-corpus-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).unwrap();

    let mut imported = 0;
    let mut degraded = 0;
    let mut rejected = 0;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();

    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy();
        let zip = path.join("archive.zip");
        let packs_root = work.join(name.as_ref());
        fs::create_dir_all(&packs_root).unwrap();
        let outcome = match import_pack(&zip, &packs_root, CollisionPolicy::Report) {
            Ok(outcome) => outcome,
            Err(error) => {
                rejected += 1;
                println!("{name}\trejected\t0\t{:?}", error.failure);
                for diag in &error.diagnostics {
                    println!("  REJECT {name} {}", diag.message);
                }
                continue;
            }
        };
        let status = if outcome.pack.characters.is_empty() {
            rejected += 1;
            "rejected"
        } else if outcome
            .diagnostics
            .iter()
            .any(|diag| diag.is_stay_fallback())
        {
            degraded += 1;
            "degraded"
        } else {
            imported += 1;
            "imported"
        };
        let mut local: BTreeMap<String, usize> = BTreeMap::new();
        for diag in &outcome.diagnostics {
            let kind = format!("{:?}", diag.kind);
            *local.entry(kind.clone()).or_default() += 1;
            *kinds.entry(kind).or_default() += 1;
        }
        let summary = local
            .iter()
            .map(|(kind, count)| format!("{kind}:{count}"))
            .collect::<Vec<_>>()
            .join(",");
        println!("{name}\t{status}\t{}\t{summary}", outcome.diagnostics.len());
        if status != "imported" {
            for diag in &outcome.diagnostics {
                println!(
                    "  NOTE {name} {:?} {}: {}",
                    diag.severity, diag.file, diag.message
                );
            }
        }
    }

    let _ = fs::remove_dir_all(&work);
    let total = imported + degraded + rejected;
    println!("TOTAL imported={imported} degraded={degraded} rejected={rejected} total={total}");
    for (kind, count) in &kinds {
        println!("KIND {kind} {count}");
    }
    assert!(total >= 50, "expected the 50-pack corpus, found {total}");
}
