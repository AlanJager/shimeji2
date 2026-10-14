//! Ignored corpus sweep. There is no awesome-codex-pets checkout on this machine.
//!
//! `SHIMEJI_CODEX_CORPUS` points at a local checkout. The test prints the load
//! rate over every pet folder and the reasons the rest are invalid. A pet whose
//! sheet is v1 or v2 and still fails to load fails the test.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use shimeji_packs::{Severity, directory_origin, load_dir};

#[test]
#[ignore = "needs SHIMEJI_CODEX_CORPUS; no awesome-codex-pets checkout on this machine"]
fn the_local_codex_corpus_reports_its_load_rate() {
    let root = std::env::var("SHIMEJI_CODEX_CORPUS")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("SHIMEJI_CODEX_CORPUS is not set"));
    let mut pets = Vec::new();
    collect(&root, &mut pets);
    pets.sort();
    let mut loaded = 0usize;
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for pet in &pets {
        let loaded_ok = load_dir(pet);
        let origin = directory_origin(pet);
        match (origin, loaded_ok) {
            (Some(_), Ok(outcome))
                if !outcome.pack.characters.is_empty()
                    && outcome
                        .diagnostics
                        .iter()
                        .all(|diag| diag.severity != Severity::Error) =>
            {
                loaded += 1;
            }
            (Some(origin), Ok(outcome)) => {
                let why = outcome
                    .diagnostics
                    .iter()
                    .find(|diag| diag.severity == Severity::Error)
                    .map(|diag| diag.message.clone())
                    .unwrap_or_else(|| "no characters".to_string());
                failures.push(format!("{origin:?} {} — {why}", pet.display()));
            }
            (Some(origin), Err(error)) => {
                failures.push(format!("{origin:?} {} — {error}", pet.display()));
            }
            (None, Ok(outcome)) => {
                let why = outcome
                    .diagnostics
                    .iter()
                    .find(|diag| diag.severity == Severity::Error)
                    .map(|diag| diag.message.clone())
                    .unwrap_or_else(|| "not a v1 or v2 Codex sheet".to_string());
                *reasons.entry(why).or_insert(0) += 1;
            }
            (None, Err(error)) => {
                *reasons.entry(error.to_string()).or_insert(0) += 1;
            }
        }
    }
    let total = pets.len();
    let rate = if total == 0 {
        0.0
    } else {
        (loaded as f64) * 100.0 / (total as f64)
    };
    eprintln!("codex corpus: loaded {loaded}/{total} = {rate:.1}%");
    eprintln!(
        "invalid {}",
        total.saturating_sub(loaded).saturating_sub(failures.len())
    );
    for (reason, count) in &reasons {
        eprintln!("  {count}  {reason}");
    }
    assert!(
        failures.is_empty(),
        "valid Codex sheets that did not load:\n{}",
        failures.join("\n")
    );
}

fn collect(dir: &Path, pets: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect(&path, pets);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("pet.json") {
            if let Some(parent) = path.parent() {
                pets.push(parent.to_path_buf());
            }
        }
    }
}
