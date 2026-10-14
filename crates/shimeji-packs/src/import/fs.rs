//! Copy a directory tree without following links.
//!
//! Only regular files and directories are copied. A symlink, FIFO, socket, or
//! device file rejects the import. Files are opened with `O_NOFOLLOW` and
//! `O_NONBLOCK`, and `fstat` must still say the fd is a regular file, so a
//! name that turns into a link or a FIFO between the check and the open cannot
//! be followed and cannot block.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;

use super::error::ImportError;
use super::limits::{MAX_DEPTH, MAX_ENTRIES, OutputBudget};

pub(crate) fn copy_tree(source: &Path, dest: &Path, pack: &str) -> Result<(), ImportError> {
    let mut budget = OutputBudget::default();
    let mut entries = 0u64;
    copy_dir(source, dest, 0, "", pack, &mut budget, &mut entries)
}

fn copy_dir(
    source: &Path,
    dest: &Path,
    depth: usize,
    rel: &str,
    pack: &str,
    budget: &mut OutputBudget,
    entries: &mut u64,
) -> Result<(), ImportError> {
    fs::create_dir_all(dest).map_err(|error| ImportError::io(pack, error))?;
    let mut names = Vec::new();
    let read = fs::read_dir(source).map_err(|error| ImportError::io(pack, error))?;
    for entry in read {
        let entry = entry.map_err(|error| ImportError::io(pack, error))?;
        names.push(entry.file_name());
    }
    names.sort();
    for name in names {
        let label = name.to_string_lossy();
        let child_rel = if rel.is_empty() {
            label.to_string()
        } else {
            format!("{rel}/{label}")
        };
        let from = source.join(&name);
        let into = dest.join(&name);
        match classify(&from)? {
            Kind::Dir => {
                count_entry(entries)?;
                if depth + 1 > MAX_DEPTH {
                    return Err(ImportError::too_deep(&child_rel).with_pack(pack));
                }
                copy_dir(&from, &into, depth + 1, &child_rel, pack, budget, entries)?;
            }
            Kind::File => {
                count_entry(entries)?;
                if depth + 1 > MAX_DEPTH {
                    return Err(ImportError::too_deep(&child_rel).with_pack(pack));
                }
                copy_file(&from, &into, pack, budget)?;
                pause_after(&child_rel);
            }
            Kind::Symlink => return Err(ImportError::symlink(child_rel).with_pack(pack)),
            Kind::Special(kind) => {
                return Err(ImportError::special(child_rel, kind).with_pack(pack));
            }
        }
    }
    Ok(())
}

fn count_entry(entries: &mut u64) -> Result<(), ImportError> {
    let next = entries.saturating_add(1);
    if next > MAX_ENTRIES {
        return Err(ImportError::entries(next));
    }
    *entries = next;
    Ok(())
}

fn copy_file(
    source: &Path,
    dest: &Path,
    pack: &str,
    budget: &mut OutputBudget,
) -> Result<(), ImportError> {
    let meta = fs::symlink_metadata(source).map_err(|error| ImportError::io(pack, error))?;
    if !meta.file_type().is_file() {
        return Err(
            ImportError::special(source.display().to_string(), "special file").with_pack(pack),
        );
    }
    budget
        .add(meta.len())
        .map_err(|error| error.with_pack(pack))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| ImportError::io(pack, error))?;
    }
    let mut input = open_nofollow(source).map_err(|error| map_open_error(pack, source, error))?;
    let opened = input
        .metadata()
        .map_err(|error| ImportError::io(pack, error))?;
    if !opened.is_file() {
        return Err(
            ImportError::special(source.display().to_string(), "special file").with_pack(pack),
        );
    }
    let mut output = create_nofollow(dest).map_err(|error| ImportError::io(pack, error))?;
    let mut buf = [0u8; 64 * 1024];
    let mut copied = 0u64;
    loop {
        let n = input
            .read(&mut buf)
            .map_err(|error| ImportError::io(pack, error))?;
        if n == 0 {
            break;
        }
        let previously = copied;
        copied += n as u64;
        if copied > meta.len() {
            let already = previously.max(meta.len());
            budget
                .add(copied - already)
                .map_err(|error| error.with_pack(pack))?;
        }
        output
            .write_all(&buf[..n])
            .map_err(|error| ImportError::io(pack, error))?;
    }
    Ok(())
}

fn map_open_error(pack: &str, source: &Path, error: io::Error) -> ImportError {
    // `O_NOFOLLOW` fails with `ELOOP` when the last component is a symlink.
    // Compare the raw code: `ErrorKind::FilesystemLoop` still needs the
    // unstable `io_error_more` feature on the toolchain this crate builds with.
    if error.raw_os_error() == Some(eloop()) {
        ImportError::symlink(source.display().to_string()).with_pack(pack)
    } else {
        ImportError::io(pack, error)
    }
}

fn eloop() -> i32 {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "l4re",
        target_os = "fuchsia"
    ))]
    {
        40
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "l4re",
        target_os = "fuchsia"
    )))]
    {
        // Darwin, the BSDs, and the illumos family.
        62
    }
}

/// Test seam. Set `SHIMEJI_PACKS_PAUSE_AFTER` to a `/`-separated path relative
/// to the source root and `SHIMEJI_PACKS_PAUSE_DIR` to a directory. After that
/// file is copied the import writes `ready` there and waits until `go` exists.
/// Unset, this returns immediately.
fn pause_after(relative: &str) {
    let Ok(needle) = std::env::var("SHIMEJI_PACKS_PAUSE_AFTER") else {
        return;
    };
    if needle != relative {
        return;
    }
    let Ok(dir) = std::env::var("SHIMEJI_PACKS_PAUSE_DIR") else {
        return;
    };
    let dir = Path::new(&dir);
    let _ = fs::write(dir.join("ready"), b"1");
    while !dir.join("go").exists() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

enum Kind {
    Dir,
    File,
    Symlink,
    Special(&'static str),
}

fn classify(path: &Path) -> Result<Kind, ImportError> {
    let meta = fs::symlink_metadata(path).map_err(|error| ImportError::io("", error))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        return Ok(Kind::Symlink);
    }
    if ft.is_dir() {
        return Ok(Kind::Dir);
    }
    if ft.is_file() {
        return Ok(Kind::File);
    }
    Ok(Kind::Special(special_name(&ft)))
}

fn special_name(ft: &fs::FileType) -> &'static str {
    #[cfg(unix)]
    {
        if ft.is_fifo() {
            return "fifo";
        }
        if ft.is_socket() {
            return "socket";
        }
        if ft.is_block_device() {
            return "block device";
        }
        if ft.is_char_device() {
            return "character device";
        }
    }
    let _ = ft;
    "special file"
}

pub(crate) fn open_nofollow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(o_nofollow() | o_nonblock());
    }
    options.open(path)
}

pub(crate) fn create_nofollow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(o_nofollow());
    }
    options.open(path)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn o_nofollow() -> i32 {
    0x0100
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn o_nonblock() -> i32 {
    0x0004
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn o_nofollow() -> i32 {
    0o400000
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn o_nonblock() -> i32 {
    0o4000
}

#[cfg(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "linux",
        target_os = "android"
    ))
))]
fn o_nofollow() -> i32 {
    0
}

#[cfg(all(
    unix,
    not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "linux",
        target_os = "android"
    ))
))]
fn o_nonblock() -> i32 {
    0
}

/// Create `path` as a real directory. A symlink at that path is an error.
pub(crate) fn ensure_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is a symlink", path.display()),
        )),
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is not a directory", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(path),
        Err(error) => Err(error),
    }
}

/// Replace a regular file. Refuses a symlink and never follows one.
pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("refusing to replace symlink {}", path.display()),
            ));
        }
        Ok(meta) if meta.is_file() => fs::remove_file(path)?,
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is not a regular file", path.display()),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut file = create_nofollow(path)?;
    file.write_all(bytes)?;
    Ok(())
}

/// Read a regular file. A symlink is an error.
pub(crate) fn read_regular(path: &Path) -> io::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    let mut file = open_nofollow(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Remove a regular file. A missing path is fine. A symlink is an error.
pub(crate) fn remove_file_nofollow(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("refusing to remove symlink {}", path.display()),
        )),
        Ok(meta) if meta.is_file() => fs::remove_file(path),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is not a regular file", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
