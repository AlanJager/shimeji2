//! Staging directory and the atomic move into `packs/<id>/`.
//!
//! On macOS a replace is `renamex_np(..., RENAME_SWAP)`, then the previous
//! tree (now sitting at the staging path) is deleted with the job directory.
//! `rename` onto a non-empty directory fails, which is why the swap exists.
//!
//! Other platforms use [`exchange_dirs_fallback`]. That renames the installed
//! directory aside, renames the staged directory into its place, then deletes
//! the aside directory. It is not atomic: a crash after the first rename leaves
//! the old pack at `<id>.replacing-<job>` and the new pack still in staging; a
//! crash after the second rename leaves the old pack in that aside directory
//! until something deletes it. The destination is never a mix of the two trees.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static JOBS: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Job {
    /// `packs_root/staging/<job>/`. Dropped on both success and failure.
    path: PathBuf,
}

impl Job {
    pub(crate) fn create(packs_root: &Path) -> io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let n = JOBS.fetch_add(1, Ordering::Relaxed);
        let path = packs_root.join("staging").join(format!("{nanos}-{n}"));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

/// Put `staged` in the place of `dest`.
///
/// `dest` must already exist and be a directory. See the module docs for the
/// macOS call and for what the portable fallback does.
pub(crate) fn replace_dir(staged: &Path, dest: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        swap_dirs(staged, dest)
    }
    #[cfg(not(target_os = "macos"))]
    {
        exchange_dirs_fallback(staged, dest)
    }
}

/// Portable replace. See the module docs.
///
/// On macOS [`replace_dir`] calls `renamex_np` instead. The function stays
/// available so the unit test below can run on every OS, including macOS.
#[cfg_attr(all(target_os = "macos", not(test)), allow(dead_code))]
pub(crate) fn exchange_dirs_fallback(staged: &Path, dest: &Path) -> io::Result<()> {
    let name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pack");
    let n = JOBS.fetch_add(1, Ordering::Relaxed);
    let aside = dest.with_file_name(format!(".{name}.replacing-{n}"));
    fs::rename(dest, &aside)?;
    if let Err(error) = fs::rename(staged, dest) {
        let _ = fs::rename(&aside, dest);
        return Err(error);
    }
    fs::remove_dir_all(&aside)?;
    Ok(())
}

#[cfg(target_os = "macos")]
use std::ffi::CString;

#[cfg(target_os = "macos")]
fn swap_dirs(staged: &Path, dest: &Path) -> io::Result<()> {
    // macOS `renamex_np`. `RENAME_SWAP` exchanges two existing directories.
    const RENAME_SWAP: u32 = 0x0000_0002;
    unsafe extern "C" {
        fn renamex_np(from: *const i8, to: *const i8, flags: u32) -> i32;
    }

    let from = c_path(staged)?;
    let to = c_path(dest)?;
    let rc = unsafe { renamex_np(from.as_ptr(), to.as_ptr(), RENAME_SWAP) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn c_path(path: &Path) -> io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains an interior NUL"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_exchange_replaces_a_non_empty_directory() {
        let root = std::env::temp_dir().join(format!(
            "shimeji-exchange-{}-{}",
            std::process::id(),
            JOBS.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("staged/keep")).unwrap();
        fs::write(root.join("staged/marker.txt"), b"new").unwrap();
        fs::create_dir_all(root.join("dest/old")).unwrap();
        fs::write(root.join("dest/marker.txt"), b"old").unwrap();

        exchange_dirs_fallback(&root.join("staged"), &root.join("dest")).unwrap();

        assert_eq!(fs::read(root.join("dest/marker.txt")).unwrap(), b"new");
        assert!(root.join("dest/keep").is_dir());
        assert!(!root.join("staged").exists());
        assert!(!root.join("dest/old").exists());
        let leftovers: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("dest")]);
        let _ = fs::remove_dir_all(&root);
    }
}
