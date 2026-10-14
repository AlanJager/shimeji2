//! Settings file: round trip, defaults, and a refused file's backup.

#[path = "support.rs"]
mod support;

use shimeji_app::settings::{self, EnvOverrides, ScaleFilter, Settings};
use shimeji_core::{DEFAULT_MASCOT_CAP, HARD_MASCOT_CAP};

#[test]
fn saving_and_loading_gives_identical_values() {
    let dir = support::scratch("round");
    let mut settings = Settings {
        scaling: 1.5,
        filter: ScaleFilter::Bicubic,
        opacity: 0.4,
        breeding: false,
        transients: false,
        transformation: false,
        throwing: false,
        sounds: true,
        multiscreen: false,
        keep_on_screen: false,
        mascot_cap: 12,
        animate_anyway: true,
        hotkey: "cmd+shift+h".to_string(),
        mischief: true,
        ..Settings::default()
    };
    settings.set_character_enabled("shime", "shime", false);
    settings.set_behavior_enabled("shime", "shime", "Wave", false);

    settings::save(&dir, &settings).unwrap();
    let loaded = settings::load(&dir);
    assert!(loaded.backup.is_none());
    assert_eq!(loaded.settings, settings);
    assert_eq!(loaded.settings.engine_settings().scaling, 1.5);
    assert!(!loaded.settings.engine_settings().breeding);
    assert!(!loaded.settings.engine_settings().keep_on_screen);
    assert!(loaded.settings.engine_settings().desktop_border_priority);
    assert!(!loaded.settings.character_enabled("shime", "shime"));
    assert!(loaded.settings.character_enabled("shime", "other"));
    assert!(!loaded.settings.behavior_enabled("shime", "shime", "Wave"));
    assert!(loaded.settings.behavior_enabled("shime", "shime", "Sit"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn agents_stay_off_when_the_key_is_absent_and_the_version_stays() {
    let dir = support::scratch("agents-default");
    std::fs::write(
        dir.join(settings::FILE_NAME),
        br#"{"version":1,"scaling":1.25,"sounds":true}"#,
    )
    .unwrap();
    let loaded = settings::load(&dir);
    assert!(loaded.backup.is_none());
    assert_eq!(loaded.settings.version, settings::VERSION);
    assert!(!loaded.settings.agents);
    assert!(loaded.settings.agent_character.is_empty());
    assert!((loaded.settings.scaling - 1.25).abs() < f64::EPSILON);
    assert!(loaded.settings.sounds);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_gives_defaults() {
    let dir = support::scratch("missing");
    let loaded = settings::load(&dir);
    assert_eq!(loaded.settings, Settings::default());
    assert!(loaded.settings.engine_settings().keep_on_screen);
    assert!(loaded.settings.engine_settings().desktop_border_priority);
    assert!(loaded.backup.is_none());
    assert!(!dir.join(settings::FILE_NAME).exists());
    assert_eq!(loaded.settings.mascot_cap, DEFAULT_MASCOT_CAP);
    assert_eq!(loaded.settings.hotkey, settings::DEFAULT_HOTKEY);
    assert!(!loaded.settings.sounds);
    assert!(!loaded.settings.animate_anyway);
    assert!(loaded.settings.keep_on_screen);
    assert!(!loaded.settings.mischief);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_file_gives_defaults_and_keeps_a_backup() {
    let dir = support::scratch("corrupt");
    let original = b"{not json";
    std::fs::write(dir.join(settings::FILE_NAME), original).unwrap();
    let loaded = settings::load(&dir);
    assert_eq!(loaded.settings, Settings::default());
    let backup = loaded.backup.expect("backup path");
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    let again = settings::load(&dir);
    assert!(again.backup.is_none());
    assert_eq!(again.settings, Settings::default());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unknown_version_gives_defaults_and_keeps_a_backup() {
    let dir = support::scratch("version");
    let original = br#"{"version":99,"scaling":4.0,"sounds":true}"#;
    std::fs::write(dir.join(settings::FILE_NAME), original).unwrap();
    let loaded = settings::load(&dir);
    assert_eq!(loaded.settings, Settings::default());
    assert_eq!(loaded.settings.scaling, 1.0);
    let backup = loaded.backup.expect("backup path");
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn out_of_range_numbers_are_clamped_on_load() {
    let dir = support::scratch("clamp");
    std::fs::write(
        dir.join(settings::FILE_NAME),
        format!(
            r#"{{"version":1,"scaling":100,"opacity":-2,"mascot_cap":{}}}"#,
            HARD_MASCOT_CAP + 50
        ),
    )
    .unwrap();
    let loaded = settings::load(&dir);
    assert!(loaded.backup.is_none());
    assert_eq!(loaded.settings.scaling, 8.0);
    assert_eq!(loaded.settings.opacity, 0.0);
    assert_eq!(loaded.settings.mascot_cap, HARD_MASCOT_CAP);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_old_settings_file_without_keep_on_screen_loads_as_true() {
    let dir = support::scratch("keep-on-screen");
    std::fs::write(
        dir.join(settings::FILE_NAME),
        r#"{"version":1,"sounds":true,"scaling":1.5}"#,
    )
    .unwrap();
    let loaded = settings::load(&dir);
    assert!(loaded.backup.is_none());
    assert!(loaded.settings.keep_on_screen);
    assert!(loaded.settings.sounds);
    assert_eq!(loaded.settings.scaling, 1.5);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_old_settings_file_without_mischief_loads_as_false() {
    let dir = support::scratch("mischief-old");
    std::fs::write(
        dir.join(settings::FILE_NAME),
        r#"{"version":1,"sounds":true,"window_rules":{"unblocked":["Terminal"],"rules":[]}}"#,
    )
    .unwrap();
    let loaded = settings::load(&dir);
    assert!(loaded.backup.is_none());
    assert!(!loaded.settings.mischief);
    assert!(loaded.settings.sounds);
    assert!(loaded.settings.window_rules.allows("Terminal", None));
    assert_eq!(loaded.settings.version, settings::VERSION);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn env_overrides_replace_animate_anyway_sound_and_the_hotkey() {
    let mut settings = Settings::default();
    assert!(!settings.sounds);
    assert!(!settings.animate_anyway);
    assert!(!settings.mischief);
    settings.apply_env(EnvOverrides {
        animate_anyway: Some("yes"),
        sound: Some("1"),
        hotkey: Some(" cmd+shift+h "),
        mischief: Some(" 1 "),
    });
    assert!(settings.animate_anyway);
    assert!(settings.sounds);
    assert_eq!(settings.hotkey, "cmd+shift+h");
    assert!(settings.mischief);
    settings.apply_env(EnvOverrides {
        animate_anyway: Some("nope"),
        sound: Some("off"),
        hotkey: None,
        mischief: Some("true"),
    });
    assert!(!settings.animate_anyway);
    assert!(!settings.sounds);
    assert_eq!(settings.hotkey, "cmd+shift+h");
    assert!(!settings.mischief);
    settings.mischief = true;
    settings.apply_env(EnvOverrides {
        animate_anyway: Some("true"),
        sound: Some("on"),
        hotkey: Some(""),
        mischief: None,
    });
    assert!(settings.animate_anyway);
    assert!(settings.sounds);
    assert_eq!(settings.hotkey, "cmd+shift+h");
    assert!(settings.mischief);
}
