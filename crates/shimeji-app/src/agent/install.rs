//! Edit Claude Code `settings.json` hooks and Codex `config.toml` `notify`.
//!
//! Every function takes the file paths it should touch. Nothing here reads
//! the real home directory. The caller passes a hook binary that lives under
//! an Applications folder and is not translocated.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use toml_edit::{Array, DocumentMut, Item};

/// Argument that marks a hook entry as ours, including after the app moves.
pub const MARKER: &str = "--shimeji2";

/// How many `*.shimeji2-bak.N` files to keep. `.1` is the newest.
const BACKUP_LIMIT: u32 = 5;

const NEW_FILE_MODE: u32 = 0o600;

/// Claude Code events the hook subscribes to.
///
/// `PermissionRequest` and `PostToolUseFailure` are separate events in the
/// current hook docs. `PostToolUse` is success-only, so it is not installed.
const CLAUDE_EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "Notification",
    "PostToolUseFailure",
    "Stop",
];

/// SHA-256 of a config file at plan time. [`FileDigest::Missing`] is a file
/// that was not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDigest {
    Missing,
    Sha256([u8; 32]),
}

/// Whether the installed hook path matches the current binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    NotInstalled,
    Current,
    OutOfDate,
}

/// Why an install or uninstall did not write.
#[derive(Debug)]
pub enum InstallError {
    Translocated {
        path: PathBuf,
    },
    OutsideApplications {
        path: PathBuf,
    },
    UnsafePath,
    MalformedClaude,
    MalformedCodex,
    ExistingNotify,
    Changed {
        claude_diff: String,
        codex_diff: String,
    },
    Io {
        path: PathBuf,
        error: io::Error,
    },
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Translocated { path } => write!(
                f,
                "{} is an App Translocation path, so hooks were not installed",
                path.display()
            ),
            Self::OutsideApplications { path } => write!(
                f,
                "{} is outside an Applications folder, so hooks were not installed",
                path.display()
            ),
            Self::UnsafePath => write!(f, "the hook path cannot be quoted safely"),
            Self::MalformedClaude => {
                write!(
                    f,
                    "settings.json is not valid JSON, so it was left unchanged"
                )
            }
            Self::MalformedCodex => {
                write!(f, "config.toml is not valid TOML, so it was left unchanged")
            }
            Self::ExistingNotify => write!(
                f,
                "config.toml already sets notify. shimeji2 will not chain another program. Remove that notify entry, then install again"
            ),
            Self::Changed { .. } => write!(
                f,
                "the hooks file changed after the diff was shown, so nothing was written"
            ),
            Self::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl std::error::Error for InstallError {}

/// A reviewed edit. Apply writes it only when the files still match the digests.
#[derive(Debug)]
pub struct InstallPlan {
    pub claude_diff: String,
    pub codex_diff: String,
    claude_digest: FileDigest,
    codex_digest: FileDigest,
    claude_next: PlannedFile,
    codex_next: PlannedFile,
}

#[derive(Debug)]
enum PlannedFile {
    Unchanged,
    Write(Vec<u8>),
}

/// Shell command for a Claude Code hook entry. The path is single-quoted.
pub fn shell_command(hook_bin: &Path) -> Result<String, InstallError> {
    let text = path_text(hook_bin)?;
    Ok(format!("{} {MARKER}", shell_quote(text)))
}

/// Refuse a translocated binary and a binary that is not under Applications.
pub fn location_block(app_path: &Path) -> Result<(), InstallError> {
    if app_path.as_os_str().is_empty() {
        return Err(InstallError::UnsafePath);
    }
    let rendered = app_path.to_string_lossy();
    if rendered.contains("/AppTranslocation/") || component_is(app_path, "AppTranslocation") {
        return Err(InstallError::Translocated {
            path: app_path.to_path_buf(),
        });
    }
    if !component_is(app_path, "Applications") {
        return Err(InstallError::OutsideApplications {
            path: app_path.to_path_buf(),
        });
    }
    Ok(())
}

/// Read both files and describe the hooks and notify edits. Writes nothing.
pub fn plan_install(
    claude: &Path,
    codex: &Path,
    hook_bin: &Path,
) -> Result<InstallPlan, InstallError> {
    location_block(hook_bin)?;
    let command = shell_command(hook_bin)?;
    let hook = path_text(hook_bin)?;
    let claude_bytes = read_optional(claude)?;
    let codex_bytes = read_optional(codex)?;
    let (claude_next, claude_diff) = plan_claude_install(claude_bytes.as_deref(), &command)?;
    let (codex_next, codex_diff) = plan_codex_install(codex_bytes.as_deref(), hook)?;
    Ok(InstallPlan {
        claude_diff,
        codex_diff,
        claude_digest: digest_of(claude_bytes.as_deref()),
        codex_digest: digest_of(codex_bytes.as_deref()),
        claude_next,
        codex_next,
    })
}

/// Write a plan when both files still have the digests from [`plan_install`].
pub fn apply_install(
    plan: &InstallPlan,
    claude: &Path,
    codex: &Path,
    hook_bin: &Path,
) -> Result<(), InstallError> {
    location_block(hook_bin)?;
    if !digests_match(plan, claude, codex)? {
        let fresh = plan_install(claude, codex, hook_bin)?;
        return Err(InstallError::Changed {
            claude_diff: fresh.claude_diff,
            codex_diff: fresh.codex_diff,
        });
    }
    commit(claude, &plan.claude_next)?;
    commit(codex, &plan.codex_next)?;
    Ok(())
}

/// Plan removal of entries that carry [`MARKER`]. Other hooks stay.
pub fn plan_uninstall(claude: &Path, codex: &Path) -> Result<InstallPlan, InstallError> {
    let claude_bytes = read_optional(claude)?;
    let codex_bytes = read_optional(codex)?;
    let (claude_next, claude_diff) = plan_claude_uninstall(claude_bytes.as_deref())?;
    let (codex_next, codex_diff) = plan_codex_uninstall(codex_bytes.as_deref())?;
    Ok(InstallPlan {
        claude_diff,
        codex_diff,
        claude_digest: digest_of(claude_bytes.as_deref()),
        codex_digest: digest_of(codex_bytes.as_deref()),
        claude_next,
        codex_next,
    })
}

/// Write an uninstall plan when the files still match its digests.
pub fn apply_uninstall(
    plan: &InstallPlan,
    claude: &Path,
    codex: &Path,
) -> Result<(), InstallError> {
    if !digests_match(plan, claude, codex)? {
        let fresh = plan_uninstall(claude, codex)?;
        return Err(InstallError::Changed {
            claude_diff: fresh.claude_diff,
            codex_diff: fresh.codex_diff,
        });
    }
    commit(claude, &plan.claude_next)?;
    commit(codex, &plan.codex_next)?;
    Ok(())
}

/// Compare installed marker entries with `hook_bin`.
///
/// A file that cannot be parsed counts as not installed. One stale marker
/// makes the result [`Freshness::OutOfDate`].
pub fn freshness(claude: &Path, codex: &Path, hook_bin: &Path) -> Freshness {
    let Ok(command) = shell_command(hook_bin) else {
        return Freshness::NotInstalled;
    };
    let Ok(hook) = path_text(hook_bin) else {
        return Freshness::NotInstalled;
    };
    let mut saw = false;
    let mut current = true;
    if let Ok(Some(bytes)) = read_optional(claude) {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                for found in claude_commands(&value) {
                    if command_has_marker(&found) {
                        saw = true;
                        if found != command {
                            current = false;
                        }
                    }
                }
            }
        }
    }
    if let Ok(Some(bytes)) = read_optional(codex) {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            if let Ok(doc) = text.parse::<DocumentMut>() {
                if let Some(item) = doc.get("notify") {
                    if notify_is_ours(item) {
                        saw = true;
                        if !notify_is_current(item, hook) {
                            current = false;
                        }
                    }
                }
            }
        }
    }
    if !saw {
        Freshness::NotInstalled
    } else if current {
        Freshness::Current
    } else {
        Freshness::OutOfDate
    }
}

fn plan_claude_install(
    bytes: Option<&[u8]>,
    command: &str,
) -> Result<(PlannedFile, String), InstallError> {
    let mut value = parse_claude(bytes)?;
    let before = hooks_pretty(&value);
    let original = value.clone();
    install_claude(&mut value, command)?;
    let after = hooks_pretty(&value);
    let planned = if value == original {
        PlannedFile::Unchanged
    } else {
        PlannedFile::Write(render_json(&value)?)
    };
    Ok((planned, diff_section("hooks", &before, &after)))
}

fn plan_claude_uninstall(bytes: Option<&[u8]>) -> Result<(PlannedFile, String), InstallError> {
    if bytes.is_none() {
        return Ok((PlannedFile::Unchanged, String::new()));
    }
    let mut value = parse_claude(bytes)?;
    let before = hooks_pretty(&value);
    let original = value.clone();
    strip_claude(&mut value)?;
    let after = hooks_pretty(&value);
    let planned = if value == original {
        PlannedFile::Unchanged
    } else {
        PlannedFile::Write(render_json(&value)?)
    };
    Ok((planned, diff_section("hooks", &before, &after)))
}

fn parse_claude(bytes: Option<&[u8]>) -> Result<Value, InstallError> {
    let Some(bytes) = bytes else {
        return Ok(Value::Object(Map::new()));
    };
    let text = std::str::from_utf8(bytes).map_err(|_| InstallError::MalformedClaude)?;
    if text.trim().is_empty() {
        return Err(InstallError::MalformedClaude);
    }
    serde_json::from_str(text).map_err(|_| InstallError::MalformedClaude)
}

fn install_claude(value: &mut Value, command: &str) -> Result<(), InstallError> {
    if !value.is_object() {
        return Err(InstallError::MalformedClaude);
    }
    {
        let root = value.as_object_mut().expect("object");
        if !root.contains_key("hooks") {
            root.insert("hooks".to_string(), Value::Object(Map::new()));
        }
    }
    if !value.get("hooks").is_some_and(Value::is_object) {
        return Err(InstallError::MalformedClaude);
    }
    for event in CLAUDE_EVENTS {
        let hooks = value
            .get_mut("hooks")
            .and_then(Value::as_object_mut)
            .expect("hooks object");
        if !hooks.contains_key(*event) {
            hooks.insert((*event).to_string(), Value::Array(Vec::new()));
        }
        let groups = hooks
            .get_mut(*event)
            .and_then(Value::as_array_mut)
            .ok_or(InstallError::MalformedClaude)?;
        upsert_groups(groups, command)?;
    }
    Ok(())
}

fn upsert_groups(groups: &mut Vec<Value>, command: &str) -> Result<(), InstallError> {
    let mut found = false;
    let mut rewritten = Vec::with_capacity(groups.len() + 1);
    for group in groups.drain(..) {
        if let Some(group) = rewrite_group(group, command, &mut found)? {
            rewritten.push(group);
        }
    }
    if !found {
        rewritten.push(our_group(command));
    }
    *groups = rewritten;
    Ok(())
}

fn rewrite_group(
    mut group: Value,
    command: &str,
    found: &mut bool,
) -> Result<Option<Value>, InstallError> {
    let Some(obj) = group.as_object_mut() else {
        return Err(InstallError::MalformedClaude);
    };
    let Some(hooks_val) = obj.get_mut("hooks") else {
        return Ok(Some(group));
    };
    let Some(hooks) = hooks_val.as_array_mut() else {
        return Err(InstallError::MalformedClaude);
    };
    let before = hooks.len();
    let mut kept = Vec::with_capacity(hooks.len());
    for hook in hooks.drain(..) {
        if !hook.is_object() {
            return Err(InstallError::MalformedClaude);
        }
        if hook_is_ours(&hook) {
            if !*found {
                let mut hook = hook;
                let hook_obj = hook.as_object_mut().expect("object");
                hook_obj.insert("type".to_string(), Value::String("command".to_string()));
                hook_obj.insert("command".to_string(), Value::String(command.to_string()));
                *found = true;
                kept.push(hook);
            }
        } else {
            kept.push(hook);
        }
    }
    let removed = kept.len() != before;
    *hooks = kept;
    if hooks.is_empty() && removed {
        return Ok(None);
    }
    Ok(Some(group))
}

fn our_group(command: &str) -> Value {
    serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": command,
        }]
    })
}

fn strip_claude(value: &mut Value) -> Result<(), InstallError> {
    if !value.is_object() {
        return Err(InstallError::MalformedClaude);
    }
    if value.get("hooks").is_none() {
        return Ok(());
    }
    if !value.get("hooks").is_some_and(Value::is_object) {
        return Err(InstallError::MalformedClaude);
    }
    let events: Vec<String> = value
        .get("hooks")
        .and_then(Value::as_object)
        .expect("hooks object")
        .keys()
        .cloned()
        .collect();
    for event in events {
        let groups = value
            .get_mut("hooks")
            .and_then(Value::as_object_mut)
            .expect("hooks object")
            .get_mut(&event)
            .and_then(Value::as_array_mut)
            .ok_or(InstallError::MalformedClaude)?;
        strip_groups(groups)?;
        if groups.is_empty() {
            value
                .get_mut("hooks")
                .and_then(Value::as_object_mut)
                .expect("hooks object")
                .remove(&event);
        }
    }
    let empty = value
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty);
    if empty {
        value.as_object_mut().expect("object").remove("hooks");
    }
    Ok(())
}

fn strip_groups(groups: &mut Vec<Value>) -> Result<(), InstallError> {
    let mut rewritten = Vec::with_capacity(groups.len());
    for group in groups.drain(..) {
        if let Some(group) = strip_group(group)? {
            rewritten.push(group);
        }
    }
    *groups = rewritten;
    Ok(())
}

fn strip_group(mut group: Value) -> Result<Option<Value>, InstallError> {
    let Some(obj) = group.as_object_mut() else {
        return Err(InstallError::MalformedClaude);
    };
    let Some(hooks_val) = obj.get_mut("hooks") else {
        return Ok(Some(group));
    };
    let Some(hooks) = hooks_val.as_array_mut() else {
        return Err(InstallError::MalformedClaude);
    };
    let before = hooks.len();
    let mut kept = Vec::with_capacity(hooks.len());
    for hook in hooks.drain(..) {
        if !hook.is_object() {
            return Err(InstallError::MalformedClaude);
        }
        if !hook_is_ours(&hook) {
            kept.push(hook);
        }
    }
    let removed = kept.len() != before;
    *hooks = kept;
    if hooks.is_empty() && removed {
        return Ok(None);
    }
    Ok(Some(group))
}

fn hook_is_ours(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(command_has_marker)
}

fn command_has_marker(command: &str) -> bool {
    command.split_whitespace().any(|word| word == MARKER)
}

fn claude_commands(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(hooks) = value.get("hooks").and_then(Value::as_object) else {
        return out;
    };
    for groups in hooks.values().filter_map(Value::as_array) {
        for group in groups.iter().filter_map(Value::as_object) {
            let Some(list) = group.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            for hook in list {
                if let Some(command) = hook.get("command").and_then(Value::as_str) {
                    out.push(command.to_string());
                }
            }
        }
    }
    out
}

fn hooks_pretty(value: &Value) -> String {
    match value.get("hooks") {
        Some(hooks) => serde_json::to_string_pretty(hooks).unwrap_or_else(|_| "(none)".to_string()),
        None => "(none)".to_string(),
    }
}

fn render_json(value: &Value) -> Result<Vec<u8>, InstallError> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|_| InstallError::MalformedClaude)?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn plan_codex_install(
    bytes: Option<&[u8]>,
    hook: &str,
) -> Result<(PlannedFile, String), InstallError> {
    let mut doc = parse_codex(bytes)?;
    if let Some(item) = doc.get("notify") {
        if !notify_is_ours(item) {
            return Err(InstallError::ExistingNotify);
        }
    }
    if doc
        .get("notify")
        .is_some_and(|item| notify_is_current(item, hook))
    {
        return Ok((PlannedFile::Unchanged, String::new()));
    }
    let before = display_notify(doc.get("notify"));
    set_notify(&mut doc, hook);
    let after = display_notify(doc.get("notify"));
    Ok((
        PlannedFile::Write(doc.to_string().into_bytes()),
        diff_section("notify", &before, &after),
    ))
}

fn plan_codex_uninstall(bytes: Option<&[u8]>) -> Result<(PlannedFile, String), InstallError> {
    if bytes.is_none() {
        return Ok((PlannedFile::Unchanged, String::new()));
    }
    let mut doc = parse_codex(bytes)?;
    let Some(item) = doc.get("notify") else {
        return Ok((PlannedFile::Unchanged, String::new()));
    };
    if !notify_is_ours(item) {
        return Ok((PlannedFile::Unchanged, String::new()));
    }
    let before = display_notify(Some(item));
    doc.remove("notify");
    Ok((
        PlannedFile::Write(doc.to_string().into_bytes()),
        diff_section("notify", &before, "(none)"),
    ))
}

fn parse_codex(bytes: Option<&[u8]>) -> Result<DocumentMut, InstallError> {
    let text = match bytes {
        None => "",
        Some(bytes) => std::str::from_utf8(bytes).map_err(|_| InstallError::MalformedCodex)?,
    };
    text.parse::<DocumentMut>()
        .map_err(|_| InstallError::MalformedCodex)
}

fn notify_is_ours(item: &Item) -> bool {
    item.as_array()
        .is_some_and(|array| array.iter().any(|value| value.as_str() == Some(MARKER)))
}

fn notify_is_current(item: &Item, hook: &str) -> bool {
    let Some(array) = item.as_array() else {
        return false;
    };
    let strings: Vec<&str> = array.iter().filter_map(toml_edit::Value::as_str).collect();
    strings.len() == array.len() && strings.as_slice() == [hook, MARKER]
}

fn set_notify(doc: &mut DocumentMut, hook: &str) {
    if let Some(array) = doc.get_mut("notify").and_then(Item::as_array_mut) {
        while !array.is_empty() {
            let _ = array.remove(0);
        }
        array.push(hook);
        array.push(MARKER);
        return;
    }
    let mut array = Array::new();
    array.push(hook);
    array.push(MARKER);
    doc["notify"] = toml_edit::value(array);
}

fn display_notify(item: Option<&Item>) -> String {
    let Some(array) = item.and_then(Item::as_array) else {
        return "(none)".to_string();
    };
    let mut fresh = Array::new();
    for value in array.iter() {
        if let Some(text) = value.as_str() {
            fresh.push(text);
        }
    }
    fresh.to_string()
}

fn diff_section(label: &str, before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }
    format!("{label}:\n--- before\n{before}\n+++ after\n{after}\n")
}

fn digests_match(plan: &InstallPlan, claude: &Path, codex: &Path) -> Result<bool, InstallError> {
    Ok(digest_file(claude)? == plan.claude_digest && digest_file(codex)? == plan.codex_digest)
}

fn digest_file(path: &Path) -> Result<FileDigest, InstallError> {
    Ok(digest_of(read_optional(path)?.as_deref()))
}

fn digest_of(bytes: Option<&[u8]>) -> FileDigest {
    match bytes {
        None => FileDigest::Missing,
        Some(bytes) => {
            let digest = Sha256::digest(bytes);
            let mut out = [0u8; 32];
            out.copy_from_slice(&digest);
            FileDigest::Sha256(out)
        }
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, InstallError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(InstallError::Io {
            path: path.to_path_buf(),
            error,
        }),
    }
}

fn commit(path: &Path, planned: &PlannedFile) -> Result<(), InstallError> {
    let PlannedFile::Write(bytes) = planned else {
        return Ok(());
    };
    let target = write_target(path)?;
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(InstallError::Io {
                path: parent.to_path_buf(),
                error: io::Error::new(io::ErrorKind::NotFound, "parent directory is missing"),
            });
        }
    }
    let mode = existing_mode(&target)?.unwrap_or(NEW_FILE_MODE);
    if target.exists() {
        let original = fs::read(&target).map_err(|error| InstallError::Io {
            path: target.clone(),
            error,
        })?;
        rotate_backup(&target, &original, mode)?;
    }
    atomic_write(&target, bytes, mode)?;
    Ok(())
}

fn write_target(path: &Path) -> Result<PathBuf, InstallError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::canonicalize(path).map_err(|error| InstallError::Io {
                path: path.to_path_buf(),
                error,
            })
        }
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(InstallError::Io {
            path: path.to_path_buf(),
            error,
        }),
    }
}

fn rotate_backup(target: &Path, original: &[u8], mode: u32) -> Result<(), InstallError> {
    let oldest = backup_path(target, BACKUP_LIMIT);
    if oldest.exists() {
        fs::remove_file(&oldest).map_err(|error| InstallError::Io {
            path: oldest.clone(),
            error,
        })?;
    }
    for n in (1..BACKUP_LIMIT).rev() {
        let src = backup_path(target, n);
        if src.exists() {
            let dst = backup_path(target, n + 1);
            fs::rename(&src, &dst).map_err(|error| InstallError::Io { path: src, error })?;
        }
    }
    write_bytes(&backup_path(target, 1), original, mode)
}

fn backup_path(target: &Path, n: u32) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!("{name}.shimeji2-bak.{n}"))
}

fn atomic_write(target: &Path, bytes: &[u8], mode: u32) -> Result<(), InstallError> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let tmp = target.with_file_name(format!(".{name}.shimeji2-tmp-{}", std::process::id()));
    if let Err(error) = write_bytes(&tmp, bytes, mode) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&tmp, target) {
        let _ = fs::remove_file(&tmp);
        return Err(InstallError::Io {
            path: target.to_path_buf(),
            error,
        });
    }
    set_mode(target, mode)
}

fn write_bytes(path: &Path, bytes: &[u8], mode: u32) -> Result<(), InstallError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options.open(path).map_err(|error| InstallError::Io {
        path: path.to_path_buf(),
        error,
    })?;
    file.write_all(bytes).map_err(|error| InstallError::Io {
        path: path.to_path_buf(),
        error,
    })?;
    set_mode(path, mode)
}

fn existing_mode(path: &Path) -> Result<Option<u32>, InstallError> {
    match fs::metadata(path) {
        Ok(meta) => Ok(Some(permission_mode(&meta))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(InstallError::Io {
            path: path.to_path_buf(),
            error,
        }),
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<(), InstallError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|error| InstallError::Io {
                path: path.to_path_buf(),
                error,
            })?
            .permissions();
        perms.set_mode(mode);
        fs::set_permissions(path, perms).map_err(|error| InstallError::Io {
            path: path.to_path_buf(),
            error,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn permission_mode(meta: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        NEW_FILE_MODE
    }
}

fn path_text(path: &Path) -> Result<&str, InstallError> {
    let text = path.to_str().ok_or(InstallError::UnsafePath)?;
    if text.is_empty() || text.chars().any(|ch| ch.is_control()) {
        return Err(InstallError::UnsafePath);
    }
    Ok(text)
}

fn shell_quote(text: &str) -> String {
    let mut out = String::from("'");
    for ch in text.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn component_is(path: &Path, name: &str) -> bool {
    path.components().any(|component| match component {
        Component::Normal(text) => text == name,
        _ => false,
    })
}
