//! Zip entry names and relative paths.
//!
//! Names are UTF-8 when the archive marks them that way, and CP932 otherwise
//! (`encoding_rs`'s Shift_JIS is the WHATWG windows-31j / CP932 decoder). The
//! text is then normalized to NFC. Absolute paths, `..`, and drive prefixes
//! never become a joined path.

use std::path::{Component, Path, PathBuf};

use encoding_rs::SHIFT_JIS;
use unicode_normalization::UnicodeNormalization;

use super::error::ImportError;
use super::limits::MAX_DEPTH;

/// Decoded zip name, not yet NFC. The duplicate error names this form.
pub(crate) fn decode_entry_text(bytes: &[u8], utf8: bool) -> Result<String, ImportError> {
    if utf8 {
        let text = std::str::from_utf8(bytes).map_err(|_| {
            ImportError::path("(zip entry)", "UTF-8 flag is set but the name is not UTF-8")
        })?;
        return Ok(text.to_string());
    }
    let (text, _, _) = SHIFT_JIS.decode(bytes);
    Ok(text.into_owned())
}

pub(crate) fn decode_entry_name(bytes: &[u8], utf8: bool) -> Result<String, ImportError> {
    Ok(decode_entry_text(bytes, utf8)?.nfc().collect())
}

/// Components under the pack root. `Err` is an entry that must not be extracted.
pub(crate) fn safe_parts(name: &str) -> Result<SafePath, ImportError> {
    if name.is_empty() || name.contains('\0') {
        return Err(ImportError::path(name, "empty or NUL in the entry name"));
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(ImportError::path(name, "absolute path"));
    }
    let directory = name.ends_with('/') || name.ends_with('\\');
    let mut parts = Vec::new();
    for part in name.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(ImportError::path(name, "parent component"));
        }
        if is_drive(part) {
            return Err(ImportError::path(name, "drive prefix"));
        }
        if part.contains('\0') {
            return Err(ImportError::path(name, "NUL in a path component"));
        }
        parts.push(part.to_string());
    }
    if parts.is_empty() {
        return Err(ImportError::path(name, "entry name has no components"));
    }
    if parts.len() > MAX_DEPTH {
        return Err(ImportError::too_deep(name));
    }
    Ok(SafePath { parts, directory })
}

pub(crate) struct SafePath {
    pub parts: Vec<String>,
    pub directory: bool,
}

pub(crate) fn join_under(root: &Path, parts: &[String]) -> PathBuf {
    let mut out = root.to_path_buf();
    for part in parts {
        out.push(part);
    }
    debug_assert!(
        out.components().all(|component| matches!(
            component,
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) | Component::CurDir
        )),
        "join_under produced a parent component"
    );
    out
}

fn is_drive(part: &str) -> bool {
    let bytes = part.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_absolute_and_drive_are_rejected() {
        assert!(safe_parts("../../evil").is_err());
        assert!(safe_parts("/etc/passwd").is_err());
        assert!(safe_parts(r"C:\windows\notepad").is_err());
        assert!(safe_parts("foo/../../evil").is_err());
    }

    #[test]
    fn dot_segments_are_dropped() {
        let path = safe_parts("./a/./b.txt").unwrap();
        assert_eq!(path.parts, vec!["a", "b.txt"]);
        assert!(!path.directory);
    }

    #[test]
    fn cp932_round_trip() {
        let (bytes, _, failed) = SHIFT_JIS.encode("動作.xml");
        assert!(!failed);
        let text = decode_entry_name(&bytes, false).unwrap();
        assert_eq!(text, "動作.xml");
    }
}
