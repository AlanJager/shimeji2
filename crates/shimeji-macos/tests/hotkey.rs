//! Hotkey grammar. Registration itself needs a running app.

use shimeji_macos::platform::hotkey::{self, parse};

#[test]
fn default_is_ctrl_shift_s() {
    let chord = hotkey::default_chord();
    assert_eq!(chord.to_string(), "ctrl+shift+s");
    assert!(chord.uses_cmd_or_ctrl());
    assert!(!chord.rejected_by_macos15());
    assert_eq!(chord.key_code, 0x01);
}

#[test]
fn macos15_rejects_option_only_and_option_shift() {
    assert!(parse("opt+s").is_err());
    assert!(parse("option+shift+s").is_err());
    assert!(parse("alt+a").is_err());
    assert!(parse("s").is_err());
    assert!(parse("shift+s").is_err());
    assert!(parse("f19").is_err());
}

#[test]
fn cmd_or_ctrl_chords_parse() {
    let cmd = parse("cmd+opt+h").unwrap();
    assert!(cmd.uses_cmd_or_ctrl());
    assert!(!cmd.rejected_by_macos15());
    assert_eq!(cmd.to_string(), "opt+cmd+h");

    let ctrl = parse("CTRL+F19").unwrap();
    assert_eq!(ctrl.key_code, 0x50);
    assert_eq!(ctrl.to_string(), "ctrl+f19");

    assert!(parse("cmd+shift+s").is_ok());
    assert!(parse("").is_err());
    assert!(parse("ctrl+ctrl").is_err());
}
