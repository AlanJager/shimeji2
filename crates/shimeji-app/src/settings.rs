//! Typed settings in `settings.json` under the app-support directory.
//!
//! Shimeji-ee fields that apply on macOS, plus the mascot cap, Animate anyway,
//! the hotkey, per-character enabled flags, and per-behavior toggles.
//! Window-mode, language, and the tray icon are not stored: the status item
//! always exists, and the UI is English. Window mischief is `mischief`, off
//! unless the user turns it on. Editable window rules are `window_rules`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use shimeji_core::{DEFAULT_MASCOT_CAP, EngineSettings, HARD_MASCOT_CAP};

use crate::window_rules::WindowRules;

/// File name inside the app-support directory.
pub const FILE_NAME: &str = "settings.json";

/// Backup of a file this process refused to load.
pub const BACKUP_NAME: &str = "settings.json.bak";

/// Written by [`save`]. An older or newer number is not loaded.
pub const VERSION: u32 = 1;

/// Carbon chord the shell registers when the user has not chosen one.
/// Control+Shift+S. The macOS shell parses this; an Option-only chord is rejected there.
pub const DEFAULT_HOTKEY: &str = "ctrl+shift+s";

const MIN_SCALING: f64 = 0.25;
const MAX_SCALING: f64 = 8.0;

fn default_keep_on_screen() -> bool {
    true
}

/// How a scaled sprite is sampled. `Hqx` is stored for Shimeji-ee compatibility;
/// the shell draws it as nearest, because the layer has no hqx filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleFilter {
    #[default]
    Nearest,
    Bicubic,
    Hqx,
}

/// One persisted configuration. Missing keys load as [`Settings::default`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// Physics scale and the layer scale. 1.0 matches the pack's pixels.
    pub scaling: f64,
    pub filter: ScaleFilter,
    /// Window alpha, 0 through 1.
    pub opacity: f64,
    pub breeding: bool,
    pub transients: bool,
    /// Persisted for the Behavior tab. The engine has no setter for it yet.
    pub transformation: bool,
    pub throwing: bool,
    /// Pack sounds. Off until the user turns them on, matching the Unit 9 mute.
    pub sounds: bool,
    pub multiscreen: bool,
    /// Draw the sprite fully inside the work area horizontally. The engine
    /// anchor still meets the wall; only the drawn sprite and its hit rect move.
    /// Missing in a version-1 file from before this field, so the default is on.
    #[serde(default = "default_keep_on_screen")]
    pub keep_on_screen: bool,
    /// Live mascots. Clamped to 1..=[`HARD_MASCOT_CAP`] on load and save.
    pub mascot_cap: usize,
    /// Overrides Reduce Motion when true.
    pub animate_anyway: bool,
    /// Hotkey spec, for example `ctrl+shift+s`. The shell parses it.
    pub hotkey: String,
    /// `pack/character` → enabled. A missing key means enabled.
    pub characters: BTreeMap<String, bool>,
    /// `pack/character` → behavior name → enabled. A missing name means enabled.
    /// Only behaviors the pack marks Toggleable are shown in the UI.
    pub behaviors: BTreeMap<String, BTreeMap<String, bool>>,
    /// Opt-in window mischief. Absent in a version-1 file from before this
    /// field, so an old file loads off. `SHIMEJI_MISCHIEF=1` can force it on
    /// for one process; that override is not written at launch.
    #[serde(default)]
    pub mischief: bool,
    /// Editable mischief rules. Absent in a version-1 file from before Unit 11.
    #[serde(default)]
    pub window_rules: WindowRules,
    /// Agent reactions. Off until the user turns them on. Absent in a
    /// version-1 file from before this field, so an old file loads off.
    #[serde(default)]
    pub agents: bool,
    /// `pack/character` that reacts. Empty uses the character the shell loaded.
    #[serde(default)]
    pub agent_character: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            scaling: 1.0,
            filter: ScaleFilter::Nearest,
            opacity: 1.0,
            breeding: true,
            transients: true,
            transformation: true,
            throwing: true,
            sounds: false,
            multiscreen: true,
            keep_on_screen: default_keep_on_screen(),
            mascot_cap: DEFAULT_MASCOT_CAP,
            animate_anyway: false,
            hotkey: DEFAULT_HOTKEY.to_string(),
            characters: BTreeMap::new(),
            behaviors: BTreeMap::new(),
            mischief: false,
            window_rules: WindowRules::default(),
            agents: false,
            agent_character: String::new(),
        }
    }
}

impl Settings {
    pub fn engine_settings(&self) -> EngineSettings {
        EngineSettings {
            scaling: self.scaling,
            breeding: self.breeding,
            transients: self.transients,
            throwing: self.throwing,
            keep_on_screen: self.keep_on_screen,
            desktop_border_priority: true,
        }
    }

    pub fn character_enabled(&self, pack: &str, character: &str) -> bool {
        self.characters
            .get(&character_key(pack, character))
            .copied()
            .unwrap_or(true)
    }

    pub fn set_character_enabled(&mut self, pack: &str, character: &str, enabled: bool) {
        self.characters
            .insert(character_key(pack, character), enabled);
    }

    pub fn behavior_enabled(&self, pack: &str, character: &str, behavior: &str) -> bool {
        self.behaviors
            .get(&character_key(pack, character))
            .and_then(|map| map.get(behavior))
            .copied()
            .unwrap_or(true)
    }

    pub fn set_behavior_enabled(
        &mut self,
        pack: &str,
        character: &str,
        behavior: &str,
        enabled: bool,
    ) {
        self.behaviors
            .entry(character_key(pack, character))
            .or_default()
            .insert(behavior.to_string(), enabled);
    }

    /// Env vars override the file for this process. Unset vars leave the field.
    /// Launch does not write the overridden values back.
    ///
    /// `1`, `true`, `yes`, and `on` are true for animate-anyway and sound. Any
    /// other present value is false. A present hotkey string replaces the spec;
    /// the shell still rejects a chord macOS will not register.
    ///
    /// Mischief is stricter: only [`mischief_from_env`] turns it on. Any other
    /// present value forces it off for this process.
    pub fn apply_env(&mut self, env: EnvOverrides<'_>) {
        if let Some(value) = env.animate_anyway {
            self.animate_anyway = truthy(value);
        }
        if let Some(value) = env.sound {
            self.sounds = truthy(value);
        }
        if let Some(value) = env.hotkey {
            let spec = value.trim();
            if !spec.is_empty() {
                self.hotkey = spec.to_string();
            }
        }
        if let Some(value) = env.mischief {
            self.mischief = mischief_from_env(value);
        }
    }

    fn sanitize(&mut self) {
        self.version = VERSION;
        if !self.scaling.is_finite() || self.scaling <= 0.0 {
            self.scaling = 1.0;
        } else {
            self.scaling = self.scaling.clamp(MIN_SCALING, MAX_SCALING);
        }
        if !self.opacity.is_finite() {
            self.opacity = 1.0;
        } else {
            self.opacity = self.opacity.clamp(0.0, 1.0);
        }
        self.mascot_cap = self.mascot_cap.clamp(1, HARD_MASCOT_CAP);
        let hotkey = self.hotkey.trim().to_string();
        self.hotkey = if hotkey.is_empty() {
            DEFAULT_HOTKEY.to_string()
        } else {
            hotkey
        };
        self.window_rules.sanitize();
    }
}

/// Present environment values. `None` means the variable is unset.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvOverrides<'a> {
    pub animate_anyway: Option<&'a str>,
    pub sound: Option<&'a str>,
    pub hotkey: Option<&'a str>,
    /// `SHIMEJI_MISCHIEF`. `None` leaves the saved switch.
    pub mischief: Option<&'a str>,
}

/// `SHIMEJI_MISCHIEF=1` after trimming. Anything else is off.
pub fn mischief_from_env(value: &str) -> bool {
    value.trim() == "1"
}

/// What [`load`] did with the file on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    /// Set when the file was refused and its bytes were copied aside.
    pub backup: Option<PathBuf>,
}

/// Read `support_dir/settings.json`.
///
/// A missing file is the default, and nothing is written. JSON that does not
/// parse, or a `version` other than [`VERSION`], loads the default and copies
/// the refused bytes to `settings.json.bak`. The refused file is then replaced
/// with the default so the next launch does not back it up again.
pub fn load(support_dir: &Path) -> Loaded {
    let path = support_dir.join(FILE_NAME);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Loaded {
                settings: Settings::default(),
                backup: None,
            };
        }
    };
    match decode(&bytes) {
        Ok(settings) => Loaded {
            settings,
            backup: None,
        },
        Err(()) => {
            let backup = support_dir.join(BACKUP_NAME);
            let wrote = fs::write(&backup, &bytes).is_ok();
            let settings = Settings::default();
            let _ = save(support_dir, &settings);
            Loaded {
                settings,
                backup: wrote.then_some(backup),
            }
        }
    }
}

/// Write `support_dir/settings.json`. Values are clamped first.
pub fn save(support_dir: &Path, settings: &Settings) -> io::Result<()> {
    let mut settings = settings.clone();
    settings.sanitize();
    let bytes = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
    let path = support_dir.join(FILE_NAME);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = support_dir.join("settings.json.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn character_key(pack: &str, character: &str) -> String {
    format!("{pack}/{character}")
}

/// `pack/character`. The pack id is the text before the first slash.
pub fn split_character_key(key: &str) -> Option<(&str, &str)> {
    let (pack, character) = key.split_once('/')?;
    if pack.is_empty() || character.is_empty() {
        None
    } else {
        Some((pack, character))
    }
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn decode(bytes: &[u8]) -> Result<Settings, ()> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    let version = value.get("version").and_then(serde_json::Value::as_u64);
    if version != Some(u64::from(VERSION)) {
        return Err(());
    }
    let mut settings: Settings = serde_json::from_value(value).map_err(|_| ())?;
    settings.sanitize();
    Ok(settings)
}
