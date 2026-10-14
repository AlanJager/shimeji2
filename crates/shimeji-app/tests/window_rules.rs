//! Window rules: immutable apps, editable defaults, title rules, EE lists.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use shimeji_app::settings::{self, Settings};
use shimeji_app::window_rules::{
    Decision, DenyReason, Effect, IMMUTABLE_APPS, Target, UserRule, WindowRules,
    parse_ee_interactive_windows,
};

fn scratch(label: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "shimeji-u11a-rules-{label}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn terminal_is_blocked_by_default_and_can_be_unblocked() {
    let rules = WindowRules::default();
    assert_eq!(
        rules.decision("Terminal", None),
        Decision::Deny(DenyReason::DefaultApp)
    );
    assert!(!rules.allows("Terminal", Some("zsh")));
    assert_eq!(
        rules.decision("1Password", None),
        Decision::Deny(DenyReason::DefaultApp)
    );
    assert!(!rules.allows("Bitwarden", None));
    assert!(!rules.allows("Keychain Access", Some("login")));
    assert!(rules.allows("TextEdit", None));

    let mut rules = WindowRules::default();
    assert!(rules.unblock_default("terminal"));
    assert!(rules.allows("Terminal", None));
    assert!(rules.allows("Terminal", Some("zsh")));
    assert!(!rules.allows("1Password", None));
    assert!(rules.block_default("Terminal"));
    assert!(!rules.allows("Terminal", None));
}

#[test]
fn system_settings_is_blocked_and_cannot_be_unblocked() {
    let mut rules = WindowRules::default();
    for app in [
        "System Settings",
        "system settings",
        "System Preferences",
        "loginwindow",
        "SecurityAgent",
        "coreautha",
    ] {
        assert_eq!(
            rules.decision(app, Some("Privacy")),
            Decision::Deny(DenyReason::Immutable),
            "{app}"
        );
        assert!(!rules.unblock_default(app));
    }

    rules.unblocked.push("System Settings".to_string());
    rules.rules.push(UserRule {
        effect: Effect::Allow,
        target: Target::App,
        text: "System Settings".to_string(),
    });
    assert_eq!(
        rules.decision("System Settings", None),
        Decision::Deny(DenyReason::Immutable)
    );
    rules.sanitize();
    assert!(!rules.unblocked.iter().any(|name| name == "System Settings"));
    assert!(!rules.allows("System Settings", Some("Privacy & Security")));
}

#[test]
fn a_title_rule_is_ignored_without_the_grant_and_applied_with_it() {
    let mut rules = WindowRules::default();
    rules.rules.push(UserRule {
        effect: Effect::Block,
        target: Target::Title,
        text: "Secret".to_string(),
    });
    assert!(rules.allows("TextEdit", None));
    assert!(!rules.allows("TextEdit", Some("Secret notes")));
    assert!(rules.allows("TextEdit", Some("Shopping")));

    let mut listed = WindowRules::default();
    listed
        .rules
        .extend(parse_ee_interactive_windows("Notes", ""));
    assert!(
        listed.allows("TextEdit", None),
        "without a title the allowlist is not in effect"
    );
    assert!(listed.allows("TextEdit", Some("My Notes")));
    assert_eq!(
        listed.decision("TextEdit", Some("Mail")),
        Decision::Deny(DenyReason::NotOnAllowlist)
    );
}

#[test]
fn ee_lists_map_onto_title_rules_and_an_old_settings_file_still_loads() {
    let parsed = parse_ee_interactive_windows("Notes//Chrome/ ", " /Password/");
    assert_eq!(
        parsed,
        vec![
            UserRule {
                effect: Effect::Block,
                target: Target::Title,
                text: "Password".to_string(),
            },
            UserRule {
                effect: Effect::Allow,
                target: Target::Title,
                text: "Notes".to_string(),
            },
            UserRule {
                effect: Effect::Allow,
                target: Target::Title,
                text: "Chrome".to_string(),
            },
        ]
    );
    let rules = WindowRules {
        rules: parsed,
        ..WindowRules::default()
    };
    assert_eq!(
        rules.decision("TextEdit", Some("Save Password")),
        Decision::Deny(DenyReason::UserBlock)
    );
    assert!(rules.allows("TextEdit", Some("Notes")));
    assert_eq!(
        rules.decision("TextEdit", Some("Mail")),
        Decision::Deny(DenyReason::NotOnAllowlist)
    );
    assert!(rules.allows("TextEdit", None));

    let mut block_only = WindowRules::default();
    block_only
        .rules
        .extend(parse_ee_interactive_windows("", "Secret"));
    assert!(block_only.allows("TextEdit", Some("Mail")));
    assert!(!block_only.allows("TextEdit", Some("Secret")));

    let dir = scratch("old");
    fs::write(
        dir.join(settings::FILE_NAME),
        r#"{"version":1,"scaling":1.25,"sounds":true}"#,
    )
    .unwrap();
    let loaded = settings::load(&dir);
    assert!(loaded.backup.is_none());
    assert_eq!(loaded.settings.version, settings::VERSION);
    assert_eq!(settings::VERSION, 1);
    assert_eq!(loaded.settings.scaling, 1.25);
    assert!(loaded.settings.sounds);
    assert_eq!(loaded.settings.window_rules, WindowRules::default());
    assert!(!loaded.settings.window_rules.allows("Terminal", None));

    let mut settings = Settings::default();
    assert!(settings.window_rules.unblock_default("Terminal"));
    settings::save(&dir, &settings).unwrap();
    let again = settings::load(&dir);
    assert!(again.backup.is_none());
    assert!(again.settings.window_rules.allows("Terminal", None));
    assert_eq!(again.settings.version, 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn add_remove_toggle_default_and_sanitize_keeps_the_immutable_list() {
    let mut rules = WindowRules::default();
    assert!(rules.set_default_allowed("Terminal", true));
    assert!(rules.default_allowed("terminal"));
    assert!(rules.allows("Terminal", None));
    assert!(rules.set_default_allowed("Terminal", false));
    assert!(!rules.default_allowed("Terminal"));
    assert!(!rules.allows("Terminal", None));
    assert!(!rules.set_default_allowed("System Settings", true));
    assert!(!rules.default_allowed("System Settings"));

    assert!(rules.add_rule(Effect::Block, Target::App, " TextEdit "));
    assert_eq!(rules.rules[0].text, "TextEdit");
    assert!(!rules.allows("TextEdit", None));
    assert!(rules.add_rule(Effect::Allow, Target::Title, "Notes"));
    assert!(!rules.add_rule(Effect::Allow, Target::App, "  "));
    assert_eq!(rules.rules.len(), 2);
    assert!(rules.remove_rule(0));
    assert_eq!(rules.rules.len(), 1);
    assert_eq!(rules.rules[0].text, "Notes");
    assert!(!rules.remove_rule(4));
    assert!(rules.allows("TextEdit", None));

    rules.unblocked.push("System Settings".to_string());
    rules.unblocked.push("loginwindow".to_string());
    rules.unblocked.push("Terminal".to_string());
    rules.sanitize();
    assert_eq!(rules.unblocked, vec!["Terminal".to_string()]);
    for app in IMMUTABLE_APPS {
        assert_eq!(
            rules.decision(app, Some("Privacy")),
            Decision::Deny(DenyReason::Immutable),
            "{app}"
        );
    }
    assert_eq!(rules.rules.len(), 1);
}
