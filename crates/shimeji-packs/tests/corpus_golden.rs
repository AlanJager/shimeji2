//! Ignored corpus sweep. Needs `corpus/traces/summary.json` from `run-corpus.sh`.
//!
//! ```
//! cargo test -p shimeji-packs --test corpus_golden -- --ignored --nocapture
//! ```

use std::fs;
use std::path::PathBuf;

use shimeji_core::diagnostics::DiagnosticKind;
use shimeji_core::model::{Character, Pack};
use shimeji_core::testing::{Scenario, first_divergence, run};
use shimeji_packs::load_dir;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct SummaryRow {
    id: String,
    result: String,
    image_set: String,
}

struct PackReport {
    id: String,
    mvp: bool,
    status: PackStatus,
}

enum PackStatus {
    Identical,
    Diverges {
        scenario: String,
        tick: u32,
        actual: String,
        expected: String,
    },
    LoadError(String),
}

fn character<'a>(pack: &'a Pack, image_set: &str) -> Result<&'a Character, String> {
    if let Some(found) = pack.character(image_set) {
        return Ok(found);
    }
    if pack.characters.len() == 1 {
        return Ok(&pack.characters[0]);
    }
    if pack.characters.is_empty() {
        return Err("load produced no characters".into());
    }
    let names: Vec<&str> = pack
        .characters
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    Err(format!(
        "image set {image_set} is not in the pack (have {})",
        names.join(", ")
    ))
}

fn scenario_names() -> Vec<String> {
    let dir = root().join("tests/scenarios");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let name = path.file_stem()?.to_str()?;
            let ext = path.extension()?.to_str()?;
            if ext == "json" && name.starts_with("shime-") {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

fn json_string_field(object: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let rest = object.split(&needle).nth(1)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code)?);
                }
                Some(other) => out.push(other),
                None => break,
            },
            '"' => return Some(out),
            _ => out.push(ch),
        }
    }
    None
}

fn json_objects(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    let mut in_string = false;
    let mut escape = false;
    for (index, ch) in text.char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(from) = start {
                        out.push(&text[from..=index]);
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }
    out
}

fn parse_summary(text: &str) -> Vec<SummaryRow> {
    json_objects(text)
        .into_iter()
        .filter_map(|object| {
            Some(SummaryRow {
                id: json_string_field(object, "id")?,
                result: json_string_field(object, "result")?,
                image_set: json_string_field(object, "imageSet")?,
            })
        })
        .collect()
}

fn load_scenario(name: &str, image_set: &str) -> Scenario {
    let path = root().join(format!("tests/scenarios/{name}.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("read {}: {error}", path.display());
    });
    let mut scenario = Scenario::parse(&text).unwrap_or_else(|error| {
        panic!("{name}: {error}");
    });
    scenario.image_set = image_set.to_string();
    scenario
}

fn run_pack(row: &SummaryRow, scenarios: &[String]) -> PackReport {
    let pack_dir = root().join("corpus/packs").join(&row.id);
    let loaded = match load_dir(&pack_dir) {
        Ok(outcome) => outcome,
        Err(error) => {
            return PackReport {
                id: row.id.clone(),
                mvp: false,
                status: PackStatus::LoadError(format!("io {error}")),
            };
        }
    };
    let mvp = !loaded
        .diagnostics
        .iter()
        .any(|diag| diag.kind == DiagnosticKind::FastFollowClass);
    let character = match character(&loaded.pack, &row.image_set) {
        Ok(item) => item,
        Err(message) => {
            return PackReport {
                id: row.id.clone(),
                mvp,
                status: PackStatus::LoadError(message),
            };
        }
    };
    for name in scenarios {
        let scenario = load_scenario(name, &row.image_set);
        let actual = run(character, &scenario);
        let golden_path = root()
            .join("corpus/traces")
            .join(&row.id)
            .join(format!("{name}.trace"));
        let golden = match fs::read_to_string(&golden_path) {
            Ok(text) => text,
            Err(error) => {
                return PackReport {
                    id: row.id.clone(),
                    mvp,
                    status: PackStatus::LoadError(format!(
                        "missing trace {}: {error}",
                        golden_path.display()
                    )),
                };
            }
        };
        if let Some(item) = first_divergence(&actual, &golden) {
            return PackReport {
                id: row.id.clone(),
                mvp,
                status: PackStatus::Diverges {
                    scenario: name.clone(),
                    tick: item.tick,
                    actual: item.actual,
                    expected: item.expected,
                },
            };
        }
    }
    PackReport {
        id: row.id.clone(),
        mvp,
        status: PackStatus::Identical,
    }
}

fn format_report(report: &PackReport) -> String {
    let mvp = if report.mvp { "mvp" } else { "fast-follow" };
    match &report.status {
        PackStatus::Identical => format!("{} | {mvp} | identical", report.id),
        PackStatus::Diverges {
            scenario,
            tick,
            actual,
            expected,
        } => format!(
            "{} | {mvp} | diverges at tick {tick} ({scenario})\n  actual   {actual}\n  golden   {expected}",
            report.id
        ),
        PackStatus::LoadError(message) => {
            format!("{} | {mvp} | load error | {message}", report.id)
        }
    }
}

#[test]
#[ignore]
fn corpus_deterministic_packs_match_mvp_tier_share() {
    let summary_path = root().join("corpus/traces/summary.json");
    let text = fs::read_to_string(&summary_path).unwrap_or_else(|error| {
        panic!(
            "read {}: {error} (generate with tools/ee-reference/run-corpus.sh)",
            summary_path.display()
        );
    });
    let rows: Vec<SummaryRow> = parse_summary(&text)
        .into_iter()
        .filter(|row| row.result == "deterministic")
        .collect();
    assert!(
        !rows.is_empty(),
        "no deterministic packs in {}",
        summary_path.display()
    );
    let scenarios = scenario_names();
    assert_eq!(
        scenarios.len(),
        7,
        "expected 7 shime scenarios, found {scenarios:?}"
    );

    let mut reports = Vec::new();
    for row in &rows {
        let report = run_pack(row, &scenarios);
        println!("{}", format_report(&report));
        reports.push(report);
    }

    let mvp: Vec<&PackReport> = reports.iter().filter(|report| report.mvp).collect();
    let identical = mvp
        .iter()
        .filter(|report| matches!(report.status, PackStatus::Identical))
        .count();
    let mvp_total = mvp.len();
    let share = if mvp_total == 0 {
        0
    } else {
        identical * 100 / mvp_total
    };
    println!("MVP-TIER identical={identical} total={mvp_total} share={share}% (target >= 90%)");
    println!(
        "ALL deterministic identical={} diverges={} load-error={} total={}",
        reports
            .iter()
            .filter(|report| matches!(report.status, PackStatus::Identical))
            .count(),
        reports
            .iter()
            .filter(|report| matches!(report.status, PackStatus::Diverges { .. }))
            .count(),
        reports
            .iter()
            .filter(|report| matches!(report.status, PackStatus::LoadError(_)))
            .count(),
        reports.len()
    );

    let table = reports
        .iter()
        .map(format_report)
        .collect::<Vec<_>>()
        .join("\n");
    let summary =
        format!("{table}\nMVP-TIER identical={identical} total={mvp_total} share={share}%\n");
    let report_path = std::env::temp_dir().join("u7-corpus-goldens.txt");
    fs::write(&report_path, &summary).ok();

    assert!(
        mvp_total > 0,
        "no MVP-tier packs among deterministic corpus rows"
    );
    assert!(
        share >= 90,
        "MVP-tier trace-identical share {identical}/{mvp_total} = {share}% is under 90%\n{table}"
    );
}
