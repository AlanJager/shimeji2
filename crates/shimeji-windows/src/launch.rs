//! Startup and bounded pack decoding, usable without a window server.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use shimeji_app::{Command, Runtime};
use shimeji_core::EngineSettings;
use shimeji_core::model::Character;
use shimeji_packs::{CollisionPolicy, DecodedImage, import_pack};

#[derive(Debug, Default)]
pub struct Options {
    pub pack: Option<PathBuf>,
    pub profile: Option<PathBuf>,
    pub character: Option<String>,
    pub scale: Option<f64>,
    pub seconds: Option<f64>,
    pub check_pack: bool,
    pub help: bool,
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--pack") => {
                    options.pack = Some(PathBuf::from(args.next().ok_or("--pack needs a path")?));
                }
                Some("--profile") => {
                    options.profile =
                        Some(PathBuf::from(args.next().ok_or("--profile needs a path")?));
                }
                Some("--character") => {
                    options.character = Some(
                        args.next()
                            .ok_or("--character needs a name")?
                            .into_string()
                            .map_err(|_| "Character name must be Unicode")?,
                    );
                }
                Some("--scale") => options.scale = Some(number(args.next(), "--scale", 0.25, 4.0)?),
                Some("--seconds") => {
                    options.seconds = Some(number(args.next(), "--seconds", 0.01, 86_400.0)?);
                }
                Some("--check-pack") => options.check_pack = true,
                Some("--help" | "-h") => options.help = true,
                Some(flag) if flag.starts_with('-') => {
                    return Err(format!("Unknown option: {flag}"));
                }
                _ if options.pack.is_none() => options.pack = Some(PathBuf::from(arg)),
                _ => return Err("Only one pack path may be supplied".to_string()),
            }
        }
        Ok(options)
    }
}

fn number(arg: Option<OsString>, name: &str, min: f64, max: f64) -> Result<f64, String> {
    let value = arg
        .and_then(|arg| arg.into_string().ok())
        .and_then(|arg| arg.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= min && *value <= max);
    value.ok_or_else(|| format!("{name} must be between {min} and {max}"))
}

pub const HELP: &str = "shimeji2 Windows preview\n\n\
    Double-click to run the bundled shime. Drag a character ZIP/folder onto the EXE,\n\
    or use the tray / pet menu to open a ZIP. Left-drag to pick up and throw.\n\n\
    shimeji2.exe [--pack PATH] [--character NAME] [--scale 0.25..4]\n\
    [--seconds N] [--profile PATH] [--check-pack] [--help]\n\n\
    Right-click the tray icon for Pomodoro focus, settings, pause, hide/show, and quit.";

/// Release packs are located relative to the EXE, never the working directory.
pub fn bundled_pack(executable: &Path) -> PathBuf {
    executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("packs/shime")
}

struct Staging(PathBuf);
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Loaded {
    pub character: Character,
    pub images: Vec<DecodedImage>,
    pub scale: f64,
}

impl Loaded {
    pub fn open(
        source: &Path,
        character_name: Option<&str>,
        scale: Option<f64>,
    ) -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "shimeji2-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path)
            .map_err(|error| format!("Cannot create import staging directory: {error}"))?;
        let staging = Staging(path);
        let imported = import_pack(source, &staging.0, CollisionPolicy::Report)
            .map_err(|error| format!("Cannot load {}:\n{error}", source.display()))?;
        let character = imported
            .pack
            .characters
            .into_iter()
            .find(|character| character_name.is_none_or(|name| character.name == name))
            .ok_or_else(|| {
                "No usable character was found. Check the pack's XML files and character name."
                    .to_string()
            })?;
        if imported.images.is_empty() {
            return Err("The pack contains no usable PNG images".to_string());
        }
        Ok(Self {
            character,
            images: imported.images,
            scale: scale
                .or(imported.display_scale)
                .unwrap_or(1.0)
                .clamp(0.25, 4.0),
        })
    }

    pub fn runtime(&self, seed: i64) -> Runtime {
        let mut runtime = Runtime::new(&self.character, seed);
        runtime.push(Command::Settings(EngineSettings {
            scaling: self.scale,
            breeding: false,
            keep_on_screen: true,
            desktop_border_priority: true,
            ..EngineSettings::default()
        }));
        runtime.push(Command::SetMascotCap(20));
        runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_paths_and_invalid_numbers() {
        let options = Options::parse([OsString::from("D:/角色 花阳.zip")]).unwrap();
        assert_eq!(options.pack, Some(PathBuf::from("D:/角色 花阳.zip")));
        for value in ["NaN", "inf", "0", "-1", "5"] {
            assert!(Options::parse(["--scale".into(), value.into()]).is_err());
        }
        assert!(Options::parse(["--pack".into()]).is_err());
        assert!(Options::parse(["--unknown".into()]).is_err());
    }

    #[test]
    fn isolated_profile_argument_is_kept_separate_from_pack_path() {
        let options = Options::parse([
            "--profile".into(),
            "D:/番茄 测试".into(),
            "D:/角色.zip".into(),
        ])
        .unwrap();
        assert_eq!(options.profile, Some(PathBuf::from("D:/番茄 测试")));
        assert_eq!(options.pack, Some(PathBuf::from("D:/角色.zip")));
        assert!(Options::parse(["--profile".into()]).is_err());
    }

    #[test]
    fn release_pack_is_relative_to_executable() {
        assert_eq!(
            bundled_pack(Path::new("/relocated/shimeji2.exe")),
            Path::new("/relocated/packs/shime")
        );
    }
}
