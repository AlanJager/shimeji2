//! Fails when `shimeji-packs` can reach a crate that builds C code.
//! Parses `Cargo.lock` directly. `cargo tree` inside `cargo test` takes the
//! same build lock and deadlocks.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
struct Package {
    name: String,
    version: String,
    deps: Vec<String>,
}

#[test]
fn shimeji_packs_has_no_c_build_dependencies() {
    let lock_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let text = fs::read_to_string(&lock_path).unwrap_or_else(|error| {
        panic!("read {}: {error}", lock_path.display());
    });
    let packages = parse_lock(&text);
    let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, package) in packages.iter().enumerate() {
        by_name
            .entry(package.name.as_str())
            .or_default()
            .push(index);
    }

    let mut queue = VecDeque::new();
    let mut seen = BTreeSet::new();
    for index in by_name.get("shimeji-packs").into_iter().flatten() {
        queue.push_back(*index);
        seen.insert(*index);
    }
    assert!(!queue.is_empty(), "shimeji-packs is not in Cargo.lock");

    let mut reached = BTreeSet::new();
    let mut via: BTreeMap<usize, String> = BTreeMap::new();
    while let Some(index) = queue.pop_front() {
        reached.insert(packages[index].name.clone());
        for spec in &packages[index].deps {
            for next in resolve(&packages, &by_name, spec) {
                if seen.insert(next) {
                    via.insert(
                        next,
                        format!(
                            "{} {} -> {spec}",
                            packages[index].name, packages[index].version
                        ),
                    );
                    queue.push_back(next);
                }
            }
        }
    }

    let mut forbidden = Vec::new();
    for index in &seen {
        let name = packages[*index].name.as_str();
        if builds_c(name) {
            let mut chain = format!("{name} {}", packages[*index].version);
            if let Some(step) = via.get(index) {
                chain.push_str(" required by ");
                chain.push_str(step);
            }
            forbidden.push(chain);
        }
    }
    assert!(
        forbidden.is_empty(),
        "shimeji-packs can reach a C build:\n{}",
        forbidden.join("\n")
    );
    assert!(
        reached.contains("flate2"),
        "walk missed flate2: {reached:?}"
    );
    assert!(reached.contains("png"));
    assert!(reached.contains("zip"));
    assert!(!reached.iter().any(|name| builds_c(name)));
}

fn builds_c(name: &str) -> bool {
    name == "cc" || name == "bindgen" || name.ends_with("-sys")
}

fn resolve(packages: &[Package], by_name: &BTreeMap<&str, Vec<usize>>, spec: &str) -> Vec<usize> {
    let mut parts = spec.split_whitespace();
    let Some(name) = parts.next() else {
        return Vec::new();
    };
    let version = parts
        .next()
        .filter(|token| token.chars().next().is_some_and(|ch| ch.is_ascii_digit()));
    let Some(candidates) = by_name.get(name) else {
        return Vec::new();
    };
    candidates
        .iter()
        .copied()
        .filter(|index| version.is_none_or(|version| packages[*index].version == version))
        .collect()
}

fn parse_lock(text: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut current: Option<Package> = None;
    let mut in_deps = false;
    for line in text.lines() {
        if line == "[[package]]" {
            if let Some(package) = current.take() {
                packages.push(package);
            }
            current = Some(Package {
                name: String::new(),
                version: String::new(),
                deps: Vec::new(),
            });
            in_deps = false;
            continue;
        }
        let Some(package) = current.as_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("name = ") {
            package.name = unquote(rest);
        } else if let Some(rest) = line.strip_prefix("version = ") {
            package.version = unquote(rest);
        } else if line.trim_start().starts_with("dependencies = [") {
            in_deps = !line.trim_end().ends_with(']');
        } else if in_deps {
            if line.trim() == "]" {
                in_deps = false;
            } else {
                let spec = line
                    .trim()
                    .trim_end_matches(',')
                    .trim()
                    .trim_matches('"')
                    .to_string();
                if !spec.is_empty() {
                    package.deps.push(spec);
                }
            }
        }
    }
    if let Some(package) = current {
        packages.push(package);
    }
    packages
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_string()
}
