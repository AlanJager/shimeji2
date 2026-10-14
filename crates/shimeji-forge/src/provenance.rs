//! Provenance written next to a generated pack.
//!
//! The prompt is omitted unless the caller passes it. Reference images are
//! sent to the provider; the notice says so on every record.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

/// Shown on every provenance record.
pub const NOTICE: &str = "Reference images are sent to the provider.";

/// Fields that go into the JSON file. `prompt: None` omits the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub model: String,
    pub date: String,
    pub forge_version: String,
    pub override_used: bool,
    pub prompt: Option<String>,
}

/// Howard Hinnant's `civil_from_days`. `0` is 1970-01-01.
pub fn date_from_unix_days(unix_days: i64) -> String {
    let z = unix_days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (yoe as i64 + era * 400) + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Today's UTC date as `YYYY-MM-DD`.
pub fn today_utc() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() / 86_400)
        .unwrap_or(0);
    date_from_unix_days(days as i64)
}

pub fn provenance_value(spec: &Provenance) -> Value {
    let mut map = Map::new();
    map.insert("model".to_string(), json!(spec.model));
    map.insert("date".to_string(), json!(spec.date));
    map.insert("forgeVersion".to_string(), json!(spec.forge_version));
    map.insert("overrideUsed".to_string(), json!(spec.override_used));
    map.insert("notice".to_string(), json!(NOTICE));
    if let Some(prompt) = &spec.prompt {
        map.insert("prompt".to_string(), json!(prompt));
    }
    Value::Object(map)
}

pub fn write_provenance(path: &Path, spec: &Provenance) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
    }
    let text =
        serde_json::to_string_pretty(&provenance_value(spec)).map_err(|err| err.to_string())?;
    fs::write(path, text).map_err(|err| err.to_string())
}
