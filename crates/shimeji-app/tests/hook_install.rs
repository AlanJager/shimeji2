//! Hook installer. Every path is under a temp directory.
//! Nothing here reads or writes the real `~/.claude` or `~/.codex`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use shimeji_app::agent::{
    Freshness, InstallError, MARKER, apply_install, apply_uninstall, freshness, plan_install,
    plan_uninstall, shell_command,
};

const SECRET: &str = "sk-test-SECRET-shimeji-u14";

struct Fixture {
    claude: PathBuf,
    codex: PathBuf,
    hook: PathBuf,
}

fn scratch(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "shimeji-u14-install-{label}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(label: &str) -> Fixture {
    let root = scratch(label);
    let claude = root.join("claude/settings.json");
    let codex = root.join("codex/config.toml");
    fs::create_dir_all(claude.parent().unwrap()).unwrap();
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    let hook = root.join("Applications/Shimeji2.app/Contents/MacOS/shimeji-hook");
    write_exec(&hook, "#!/bin/sh\nexit 0\n");
    Fixture {
        claude,
        codex,
        hook,
    }
}

fn write_0600(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).unwrap().write_all(bytes).unwrap();
    set_mode(path, 0o600);
}

fn write_exec(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    set_mode(path, 0o755);
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).unwrap();
}

#[cfg(not(unix))]
fn set_mode(path: &Path, mode: u32) {
    let _ = (path, mode);
}

fn install(fix: &Fixture) {
    let plan = plan_install(&fix.claude, &fix.codex, &fix.hook).unwrap();
    apply_install(&plan, &fix.claude, &fix.codex, &fix.hook).unwrap();
}

fn uninstall(fix: &Fixture) {
    let plan = plan_uninstall(&fix.claude, &fix.codex).unwrap();
    apply_uninstall(&plan, &fix.claude, &fix.codex).unwrap();
}

fn claude_value(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn commands(path: &Path) -> Vec<String> {
    let value = claude_value(path);
    let mut out = Vec::new();
    let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) else {
        return out;
    };
    for groups in hooks.values().filter_map(|v| v.as_array()) {
        for group in groups.iter().filter_map(|v| v.as_object()) {
            let Some(list) = group.get("hooks").and_then(|v| v.as_array()) else {
                continue;
            };
            for hook in list {
                if let Some(command) = hook.get("command").and_then(|v| v.as_str()) {
                    out.push(command.to_string());
                }
            }
        }
    }
    out
}

fn applications_dir(hook: &Path) -> PathBuf {
    hook.parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn user_settings() -> String {
    format!(
        r#"{{
  "env": {{"ANTHROPIC_API_KEY": "{SECRET}"}},
  "hooks": {{
    "PreToolUse": [
      {{
        "matcher": "Bash",
        "hooks": [{{"type": "command", "command": "echo user-hook"}}]
      }}
    ]
  }}
}}
"#
    )
}

#[test]
fn install_keeps_existing_user_hooks_and_hides_other_keys() {
    let fix = fixture("keep");
    let original = user_settings();
    write_0600(&fix.claude, original.as_bytes());
    write_0600(
        &fix.codex,
        format!("# keep-this-comment\napi_key = \"{SECRET}\"\nmodel = \"gpt-5\"\n").as_bytes(),
    );
    assert_eq!(
        freshness(&fix.claude, &fix.codex, &fix.hook),
        Freshness::NotInstalled
    );
    let before_hooks = claude_value(&fix.claude)["hooks"].clone();
    let plan = plan_install(&fix.claude, &fix.codex, &fix.hook).unwrap();
    assert!(plan.claude_diff.contains(MARKER), "{}", plan.claude_diff);
    assert!(
        plan.claude_diff.contains("echo user-hook"),
        "{}",
        plan.claude_diff
    );
    assert!(!plan.claude_diff.contains(SECRET), "{}", plan.claude_diff);
    assert!(plan.codex_diff.contains(MARKER), "{}", plan.codex_diff);
    assert!(!plan.codex_diff.contains(SECRET), "{}", plan.codex_diff);
    assert!(!plan.codex_diff.contains("api_key"), "{}", plan.codex_diff);
    apply_install(&plan, &fix.claude, &fix.codex, &fix.hook).unwrap();

    let found = commands(&fix.claude);
    assert_eq!(found.iter().filter(|c| *c == "echo user-hook").count(), 1);
    assert!(found.iter().any(|c| c.contains(MARKER)));
    let installed = claude_value(&fix.claude);
    assert_eq!(installed["env"]["ANTHROPIC_API_KEY"], SECRET);
    assert!(fs::read_to_string(&fix.codex).unwrap().contains("api_key"));
    assert!(fs::read_to_string(&fix.codex).unwrap().contains(MARKER));

    let once = fs::read(&fix.claude).unwrap();
    let codex_once = fs::read(&fix.codex).unwrap();
    install(&fix);
    assert_eq!(fs::read(&fix.claude).unwrap(), once);
    assert_eq!(fs::read(&fix.codex).unwrap(), codex_once);

    uninstall(&fix);
    assert_eq!(claude_value(&fix.claude)["hooks"], before_hooks);
    assert_eq!(
        claude_value(&fix.claude)["env"]["ANTHROPIC_API_KEY"],
        SECRET
    );
    assert!(!commands(&fix.claude).iter().any(|c| c.contains(MARKER)));
    let codex = fs::read_to_string(&fix.codex).unwrap();
    assert!(!codex.contains(MARKER));
    assert!(codex.contains(&format!("api_key = \"{SECRET}\"")) || codex.contains(SECRET));
}

#[test]
#[cfg(unix)]
fn a_backup_keeps_the_original_bytes_and_mode_0600() {
    let fix = fixture("backup");
    let original = user_settings();
    write_0600(&fix.claude, original.as_bytes());
    let toml = b"# keep-this-comment\nmodel = \"gpt-5\"\n";
    write_0600(&fix.codex, toml);
    install(&fix);
    let claude_bak = fix.claude.with_file_name("settings.json.shimeji2-bak.1");
    let codex_bak = fix.codex.with_file_name("config.toml.shimeji2-bak.1");
    use std::os::unix::fs::MetadataExt;
    assert_eq!(fs::read(&claude_bak).unwrap(), original.as_bytes());
    assert_eq!(fs::read(&codex_bak).unwrap(), toml);
    assert_eq!(fs::metadata(&claude_bak).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&codex_bak).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&fix.claude).unwrap().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&fix.codex).unwrap().mode() & 0o777, 0o600);
}

#[test]
fn malformed_settings_json_is_left_unchanged() {
    let fix = fixture("malformed");
    let original = b"{";
    write_0600(&fix.claude, original);
    let toml = b"model = \"gpt-5\"\n";
    write_0600(&fix.codex, toml);
    let err = plan_install(&fix.claude, &fix.codex, &fix.hook).unwrap_err();
    assert!(matches!(err, InstallError::MalformedClaude));
    assert_eq!(fs::read(&fix.claude).unwrap(), original);
    assert_eq!(fs::read(&fix.codex).unwrap(), toml);
}

#[test]
fn an_app_move_replaces_the_marked_entry_and_leaves_the_user_hook() {
    let fix = fixture("move");
    write_0600(&fix.claude, user_settings().as_bytes());
    write_0600(&fix.codex, b"model = \"gpt-5\"\n");
    install(&fix);
    let new_hook = applications_dir(&fix.hook).join("Moved App.app/Contents/MacOS/shimeji-hook");
    write_exec(&new_hook, "#!/bin/sh\nexit 0\n");
    assert_eq!(
        freshness(&fix.claude, &fix.codex, &new_hook),
        Freshness::OutOfDate
    );
    let plan = plan_install(&fix.claude, &fix.codex, &new_hook).unwrap();
    apply_install(&plan, &fix.claude, &fix.codex, &new_hook).unwrap();
    let commands = commands(&fix.claude);
    assert_eq!(
        commands.iter().filter(|c| *c == "echo user-hook").count(),
        1
    );
    let ours: Vec<_> = commands.iter().filter(|c| c.contains(MARKER)).collect();
    assert_eq!(ours.len(), 6);
    assert!(ours.iter().all(|c| c.contains("Moved App.app")));
    assert!(ours.iter().all(|c| !c.contains("Shimeji2.app")));
    assert_eq!(
        freshness(&fix.claude, &fix.codex, &new_hook),
        Freshness::Current
    );
    let codex = fs::read_to_string(&fix.codex).unwrap();
    assert!(codex.contains("Moved App.app"));
    assert!(!codex.contains("Shimeji2.app"));
}

#[test]
#[cfg(unix)]
fn a_path_with_a_space_and_a_quote_produces_a_command_that_runs() {
    let root = scratch("quote");
    let hook = root.join("Applications/O'Brien Apps/Shimeji 2.app/Contents/MacOS/shimeji-hook");
    let sentinel = root.join("sentinel");
    write_exec(
        &hook,
        "#!/bin/sh\nif [ \"$1\" = \"--shimeji2\" ]; then printf ran > \"$SHIMEJI_SENTINEL\"; exit 0; fi\nexit 2\n",
    );
    let command = shell_command(&hook).unwrap();
    assert!(command.contains("'\\''"), "{command}");
    assert!(command.ends_with(&format!(" {MARKER}")), "{command}");
    let status = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .env("SHIMEJI_SENTINEL", &sentinel)
        .status()
        .unwrap();
    assert!(status.success(), "{status} from {command}");
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "ran");

    let claude = root.join("claude/settings.json");
    let codex = root.join("codex/config.toml");
    fs::create_dir_all(claude.parent().unwrap()).unwrap();
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    write_0600(&claude, b"{}\n");
    write_0600(&codex, b"");
    let plan = plan_install(&claude, &codex, &hook).unwrap();
    apply_install(&plan, &claude, &codex, &hook).unwrap();
    let stored = commands(&claude).into_iter().next().unwrap();
    let _ = fs::remove_file(&sentinel);
    let status = Command::new("sh")
        .arg("-c")
        .arg(&stored)
        .env("SHIMEJI_SENTINEL", &sentinel)
        .status()
        .unwrap();
    assert!(status.success(), "{status} from stored {stored}");
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "ran");
}

#[test]
fn a_translocated_or_outside_path_is_refused() {
    let fix = fixture("where");
    write_0600(&fix.claude, b"{}\n");
    write_0600(&fix.codex, b"");
    let before_c = fs::read(&fix.claude).unwrap();
    let before_x = fs::read(&fix.codex).unwrap();
    let translocated = PathBuf::from(
        "/private/var/folders/ab/AppTranslocation/XYZ/d/Applications/Shimeji2.app/Contents/MacOS/shimeji-hook",
    );
    let err = plan_install(&fix.claude, &fix.codex, &translocated).unwrap_err();
    assert!(matches!(err, InstallError::Translocated { .. }));
    let outside = PathBuf::from("/tmp/shimeji-hook");
    let err = plan_install(&fix.claude, &fix.codex, &outside).unwrap_err();
    assert!(matches!(err, InstallError::OutsideApplications { .. }));
    assert_eq!(fs::read(&fix.claude).unwrap(), before_c);
    assert_eq!(fs::read(&fix.codex).unwrap(), before_x);
}

#[test]
fn a_file_that_changes_after_the_diff_aborts() {
    let fix = fixture("race");
    write_0600(&fix.claude, user_settings().as_bytes());
    write_0600(&fix.codex, format!("api_key = \"{SECRET}\"\n").as_bytes());
    let plan = plan_install(&fix.claude, &fix.codex, &fix.hook).unwrap();
    let mutated = user_settings().replace("echo user-hook", "echo USER_HOOK_77");
    write_0600(&fix.claude, mutated.as_bytes());
    let err = apply_install(&plan, &fix.claude, &fix.codex, &fix.hook).unwrap_err();
    match err {
        InstallError::Changed {
            claude_diff,
            codex_diff,
        } => {
            assert!(claude_diff.contains("USER_HOOK_77"), "{claude_diff}");
            assert!(claude_diff.contains(MARKER), "{claude_diff}");
            assert!(!claude_diff.contains(SECRET), "{claude_diff}");
            assert!(!codex_diff.contains(SECRET), "{codex_diff}");
        }
        other => panic!("expected Changed, got {other}"),
    }
    assert_eq!(fs::read(&fix.claude).unwrap(), mutated.as_bytes());
    assert!(!fs::read_to_string(&fix.codex).unwrap().contains(MARKER));
}

#[test]
#[cfg(unix)]
fn a_symlinked_settings_file_stays_a_symlink() {
    let root = scratch("symlink");
    let real_claude = root.join("real/settings.json");
    let real_codex = root.join("real/config.toml");
    let link_claude = root.join("link/settings.json");
    let link_codex = root.join("link/config.toml");
    write_0600(&real_claude, user_settings().as_bytes());
    write_0600(&real_codex, b"# keep-this-comment\nmodel = \"gpt-5\"\n");
    fs::create_dir_all(link_claude.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real_claude, &link_claude).unwrap();
    std::os::unix::fs::symlink(&real_codex, &link_codex).unwrap();
    let hook = root.join("Applications/Shimeji2.app/Contents/MacOS/shimeji-hook");
    write_exec(&hook, "#!/bin/sh\nexit 0\n");
    let plan = plan_install(&link_claude, &link_codex, &hook).unwrap();
    apply_install(&plan, &link_claude, &link_codex, &hook).unwrap();
    assert!(
        fs::symlink_metadata(&link_claude)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::symlink_metadata(&link_codex)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&link_claude).unwrap(), real_claude);
    assert_eq!(fs::read_link(&link_codex).unwrap(), real_codex);
    assert!(fs::read_to_string(&real_claude).unwrap().contains(MARKER));
    assert!(fs::read_to_string(&real_codex).unwrap().contains(MARKER));
    assert!(
        real_claude
            .with_file_name("settings.json.shimeji2-bak.1")
            .is_file()
    );
    assert!(
        !link_claude
            .with_file_name("settings.json.shimeji2-bak.1")
            .exists()
    );
}

#[test]
fn an_existing_codex_notify_is_refused_and_left_unchanged() {
    let fix = fixture("notify");
    write_0600(&fix.claude, user_settings().as_bytes());
    let toml = b"notify = [\"other-program\"]\n# keep-this-comment\n";
    write_0600(&fix.codex, toml);
    let claude_before = fs::read(&fix.claude).unwrap();
    let err = plan_install(&fix.claude, &fix.codex, &fix.hook).unwrap_err();
    assert!(matches!(err, InstallError::ExistingNotify));
    assert_eq!(fs::read(&fix.claude).unwrap(), claude_before);
    assert_eq!(fs::read(&fix.codex).unwrap(), toml);

    let plan = plan_uninstall(&fix.claude, &fix.codex).unwrap();
    apply_uninstall(&plan, &fix.claude, &fix.codex).unwrap();
    assert_eq!(fs::read(&fix.codex).unwrap(), toml);
}

#[test]
fn toml_comments_survive_a_round_trip() {
    let fix = fixture("comments");
    write_0600(&fix.claude, b"{}\n");
    let toml = "# keep-this-comment\nmodel = \"gpt-5\"\n\n# section-note\n[tui]\nnotifications = [\"agent-turn-complete\"]\n";
    write_0600(&fix.codex, toml.as_bytes());
    install(&fix);
    let installed = fs::read_to_string(&fix.codex).unwrap();
    assert!(installed.contains("# keep-this-comment"), "{installed}");
    assert!(installed.contains("# section-note"), "{installed}");
    assert!(installed.contains("agent-turn-complete"), "{installed}");
    assert!(installed.contains(MARKER), "{installed}");
    uninstall(&fix);
    let removed = fs::read_to_string(&fix.codex).unwrap();
    assert!(removed.contains("# keep-this-comment"), "{removed}");
    assert!(removed.contains("# section-note"), "{removed}");
    assert!(removed.contains("agent-turn-complete"), "{removed}");
    assert!(!removed.contains(MARKER), "{removed}");
}

#[test]
fn backups_stay_bounded_at_five() {
    let fix = fixture("bound");
    write_0600(&fix.claude, b"{}\n");
    write_0600(&fix.codex, b"model = \"gpt-5\"\n");
    let apps = applications_dir(&fix.hook);
    for n in 1..=6 {
        let hook = apps.join(format!("App{n}.app/Contents/MacOS/shimeji-hook"));
        write_exec(&hook, "#!/bin/sh\nexit 0\n");
        let plan = plan_install(&fix.claude, &fix.codex, &hook).unwrap();
        apply_install(&plan, &fix.claude, &fix.codex, &hook).unwrap();
    }
    for n in 1..=5 {
        assert!(
            fix.claude
                .with_file_name(format!("settings.json.shimeji2-bak.{n}"))
                .is_file(),
            "missing bak.{n}"
        );
        assert!(
            fix.codex
                .with_file_name(format!("config.toml.shimeji2-bak.{n}"))
                .is_file(),
            "missing toml bak.{n}"
        );
    }
    assert!(
        !fix.claude
            .with_file_name("settings.json.shimeji2-bak.6")
            .exists()
    );
    assert!(
        !fix.codex
            .with_file_name("config.toml.shimeji2-bak.6")
            .exists()
    );
}
