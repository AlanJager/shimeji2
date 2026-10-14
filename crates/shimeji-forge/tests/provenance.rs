//! The prompt field exists only when the caller asks to share it.

use shimeji_forge::{NOTICE, Provenance, date_from_unix_days, provenance_value};

fn sample(prompt: Option<&str>) -> Provenance {
    Provenance {
        model: "test-model".to_string(),
        date: "2026-10-10".to_string(),
        forge_version: "0.1.0".to_string(),
        override_used: true,
        prompt: prompt.map(str::to_string),
    }
}

#[test]
fn unix_day_zero_and_today_are_civil_dates() {
    assert_eq!(date_from_unix_days(0), "1970-01-01");
    assert_eq!(date_from_unix_days(20736), "2026-10-10");
}

#[test]
fn provenance_omits_the_prompt_unless_it_was_shared() {
    let hidden = provenance_value(&sample(None));
    assert!(hidden.get("prompt").is_none(), "{hidden}");
    assert_eq!(hidden["model"], "test-model");
    assert_eq!(hidden["date"], "2026-10-10");
    assert_eq!(hidden["forgeVersion"], "0.1.0");
    assert_eq!(hidden["overrideUsed"], true);
    assert_eq!(hidden["notice"], NOTICE);

    let shared = provenance_value(&sample(Some("a mascot waving")));
    assert_eq!(shared["prompt"], "a mascot waving");
    assert_eq!(shared["notice"], NOTICE);

    let empty = provenance_value(&sample(Some("")));
    assert_eq!(empty["prompt"], "");
    assert!(empty.get("notice").is_some());
}
