//! File-based agent mode. Forge writes `jobs.json`. An external agent writes the
//! image files. There is no socket and no use of `shimeji-proto`.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{GenerateRequest, ImageProvider, ProviderError};
use crate::jobs::{self, GROUPS, Group};
use crate::raster::{self, RgbaImage};
use crate::templates::{self, pose_text};

#[derive(Debug, Clone)]
pub struct AgentMode {
    pub dir: PathBuf,
    pub timeout: Duration,
}

impl AgentMode {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            timeout: Duration::from_secs(0),
        }
    }

    pub fn write_jobs(&self, character: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.dir).map_err(|err| err.to_string())?;
        let guides = self.dir.join("guides");
        fs::create_dir_all(&guides).map_err(|err| err.to_string())?;
        let guide_src = templates::pose_guides_dir();
        let mut jobs = Vec::new();
        for group in GROUPS {
            if guide_src.join("shime1.png").is_file() {
                if let Ok(strip) = templates::guide_strip(&guide_src, group.frames) {
                    let _ = raster::save_png(&guides.join(format!("{}.png", group.id)), &strip);
                }
            }
            jobs.push(job_value(group, &self.dir));
        }
        let body = json!({
            "version": 1,
            "mode": "agent",
            "character": character,
            "background": "#00FF00",
            "jobs": jobs,
        });
        let path = self.dir.join("jobs.json");
        let text = serde_json::to_string_pretty(&body).map_err(|err| err.to_string())?;
        fs::write(&path, text).map_err(|err| err.to_string())?;
        Ok(path)
    }

    pub fn output_paths(&self) -> Result<Vec<PathBuf>, String> {
        let path = self.dir.join("jobs.json");
        let text = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let value: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
        let jobs = value
            .get("jobs")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "jobs.json has no jobs".to_string())?;
        let mut paths = Vec::new();
        for job in jobs {
            let name = job
                .get("output")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "job is missing output".to_string())?;
            paths.push(self.dir.join(name));
        }
        Ok(paths)
    }

    /// Poll until every output file exists, or the timeout elapses.
    pub fn wait_for_outputs(&self) -> Result<(), String> {
        let paths = self.output_paths()?;
        let start = Instant::now();
        loop {
            let missing: Vec<_> = paths
                .iter()
                .filter(|path| !path.is_file())
                .cloned()
                .collect();
            if missing.is_empty() {
                return Ok(());
            }
            if start.elapsed() >= self.timeout {
                let names: Vec<_> = missing
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect();
                return Err(format!(
                    "image files have not appeared: {}",
                    names.join(", ")
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn job_value(group: &Group, dir: &Path) -> Value {
    let notes: Vec<Value> = group
        .frames
        .iter()
        .enumerate()
        .map(|(slot, index)| {
            json!({
                "slot": slot + 1,
                "frame": index,
                "text": pose_text(*index).unwrap_or(""),
            })
        })
        .collect();
    json!({
        "id": group.id,
        "group": group.id,
        "title": group.title,
        "frames": group.frames,
        "slots": group.frames.len(),
        "standing": group.standing,
        "prompt": jobs::prompts_dir().join(format!("groups/{}.md", group.id)),
        "base_prompt": jobs::prompts_dir().join("base.md"),
        "strip_prompt": jobs::prompts_dir().join("strip.md"),
        "guide": dir.join("guides").join(format!("{}.png", group.id)),
        "output": format!("{}.png", group.id),
        "notes": notes,
    })
}

impl ImageProvider for AgentMode {
    fn name(&self) -> &'static str {
        "agent"
    }

    fn generate(&self, request: &GenerateRequest) -> Result<Vec<RgbaImage>, ProviderError> {
        let _ = request;
        self.write_jobs("character")
            .map_err(ProviderError::Message)?;
        self.wait_for_outputs().map_err(ProviderError::Message)?;
        let paths = self.output_paths().map_err(ProviderError::Message)?;
        let mut images = Vec::new();
        for path in paths {
            images.push(raster::decode_png(&path).map_err(ProviderError::Message)?);
        }
        Ok(images)
    }
}
