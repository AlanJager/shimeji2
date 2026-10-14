//! Every `${}` / `#{}` script in the bundled shime XML must parse.

use shimeji_expr::parse_script;
use std::fs;
use std::path::{Path, PathBuf};

fn conf_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime/conf")
}

fn extract_scripts(xml: &str) -> Vec<String> {
    let xml = strip_comments(xml);
    let mut found = Vec::new();
    let mut index = 0;
    let bytes = xml.as_str();
    while index < bytes.len() {
        let rest = &bytes[index..];
        let dollar = rest.find("${").map(|at| index + at);
        let hash = rest.find("#{").map(|at| index + at);
        let start = match (dollar, hash) {
            (None, None) => break,
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (Some(a), Some(b)) => a.min(b),
        };
        let mut cursor = start + 2;
        let mut depth = 1;
        while cursor < bytes.len() && depth > 0 {
            match bytes.as_bytes()[cursor] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            cursor += 1;
        }
        if depth != 0 {
            break;
        }
        let interior = unescape_xml(&bytes[start + 2..cursor - 1]);
        let trimmed = interior.trim();
        if !trimmed.is_empty() {
            found.push(trimmed.to_string());
        }
        index = cursor;
    }
    found
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if let Some(rel) = text[index..].find("<!--") {
            out.push_str(&text[index..index + rel]);
            let after = index + rel + 4;
            match text[after..].find("-->") {
                Some(end) => index = after + end + 3,
                None => break,
            }
        } else {
            out.push_str(&text[index..]);
            break;
        }
    }
    out
}

fn unescape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if let Some(rel) = text[index..].find('&') {
            out.push_str(&text[index..index + rel]);
            let amp = index + rel;
            match text[amp + 1..].find(';') {
                None => {
                    out.push_str(&text[amp..]);
                    break;
                }
                Some(semi_rel) => {
                    let entity = &text[amp + 1..amp + 1 + semi_rel];
                    match entity {
                        "lt" => out.push('<'),
                        "gt" => out.push('>'),
                        "amp" => out.push('&'),
                        "quot" => out.push('"'),
                        "apos" => out.push('\''),
                        other if other.starts_with("#x") || other.starts_with("#X") => {
                            if let Ok(code) = u32::from_str_radix(&other[2..], 16) {
                                if let Some(ch) = char::from_u32(code) {
                                    out.push(ch);
                                }
                            }
                        }
                        other if other.starts_with('#') => {
                            if let Ok(code) = other[1..].parse::<u32>() {
                                if let Some(ch) = char::from_u32(code) {
                                    out.push(ch);
                                }
                            }
                        }
                        _ => {
                            out.push('&');
                            out.push_str(entity);
                            out.push(';');
                        }
                    }
                    index = amp + 1 + semi_rel + 1;
                }
            }
        } else {
            out.push_str(&text[index..]);
            break;
        }
    }
    out
}

fn load_xml(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    String::from_utf8(bytes).unwrap_or_else(|error| panic!("utf8 {}: {error}", path.display()))
}

#[test]
fn every_bundled_shime_expression_parses() {
    let dir = conf_dir();
    let files = ["動作.xml", "行動.xml"];
    let mut failures = Vec::new();
    let mut total = 0usize;
    for name in files {
        let path = dir.join(name);
        let xml = load_xml(&path);
        let scripts = extract_scripts(&xml);
        assert!(
            !scripts.is_empty(),
            "{} should contain at least one script",
            path.display()
        );
        for expr in scripts {
            total += 1;
            if let Err(error) = parse_script(&expr) {
                failures.push(format!("{}: {error} :: {}", name, collapse(&expr)));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{}/{} failed:\n{}",
        failures.len(),
        total,
        failures.join("\n")
    );
    assert!(total >= 80, "expected dozens of shime scripts, got {total}");
}

fn collapse(src: &str) -> String {
    src.split_whitespace().collect::<Vec<_>>().join(" ")
}
