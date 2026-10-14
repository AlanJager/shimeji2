//! Socket path for the per-user hook transport.
//!
//! The path is `{home}/Library/Application Support/shimeji2/run/sock`. `home`
//! is the passwd entry for the real uid. `TMPDIR`, `HOME`, and every other
//! environment variable are ignored.
//!
//! The `run` directory must be a real directory (not a link), owned by the
//! effective uid, mode `0700`. The listener unlinks a stale socket only when
//! that socket is a socket we own. A regular file is left in place and
//! reported. The hook runs the same directory checks and does not unlink.

use std::io;
use std::path::{Path, PathBuf};

/// Who is about to use the socket path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketRole {
    /// Check the directory and replace a stale socket we own, so `bind` can proceed.
    Listener,
    /// Check the directory and leave the socket path untouched.
    Client,
}

/// Why the socket directory cannot be used. The display text is the reason.
#[derive(Debug)]
pub enum SocketError {
    Unsupported,
    NoHome,
    Symlink { path: PathBuf },
    NotDirectory { path: PathBuf },
    WrongMode { path: PathBuf, mode: u32 },
    WrongOwner { path: PathBuf },
    RegularFile { path: PathBuf },
    ForeignSocket { path: PathBuf },
    Io { path: PathBuf, error: io::Error },
}

impl PartialEq for SocketError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Unsupported, Self::Unsupported) | (Self::NoHome, Self::NoHome) => true,
            (Self::Symlink { path: a }, Self::Symlink { path: b })
            | (Self::NotDirectory { path: a }, Self::NotDirectory { path: b })
            | (Self::WrongOwner { path: a }, Self::WrongOwner { path: b })
            | (Self::RegularFile { path: a }, Self::RegularFile { path: b })
            | (Self::ForeignSocket { path: a }, Self::ForeignSocket { path: b }) => a == b,
            (Self::WrongMode { path: a, mode: am }, Self::WrongMode { path: b, mode: bm }) => {
                a == b && am == bm
            }
            (Self::Io { path: a, error: ae }, Self::Io { path: b, error: be }) => {
                a == b && ae.kind() == be.kind()
            }
            _ => false,
        }
    }
}

impl Eq for SocketError {}

impl std::fmt::Display for SocketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(f, "the agent socket is only available on Unix"),
            Self::NoHome => write!(f, "the home directory for this user could not be read"),
            Self::Symlink { path } => write!(
                f,
                "{} is a symbolic link; the agent socket will not follow it",
                path.display()
            ),
            Self::NotDirectory { path } => {
                write!(f, "{} is not a directory", path.display())
            }
            Self::WrongMode { path, mode } => write!(
                f,
                "{} must be mode 0700 (found {:04o})",
                path.display(),
                mode
            ),
            Self::WrongOwner { path } => {
                write!(f, "{} is not owned by this user", path.display())
            }
            Self::RegularFile { path } => write!(
                f,
                "{} is a regular file and was left in place",
                path.display()
            ),
            Self::ForeignSocket { path } => {
                write!(f, "{} is a socket owned by another user", path.display())
            }
            Self::Io { path, error } => {
                write!(f, "{}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for SocketError {}

const PARTS: [&str; 4] = ["Library", "Application Support", "shimeji2", "run"];
const SOCKET_NAME: &str = "sock";

/// `{home}/Library/Application Support/shimeji2/run/sock`.
pub fn socket_path(home: &Path) -> PathBuf {
    let mut path = home.to_path_buf();
    for part in PARTS {
        path.push(part);
    }
    path.push(SOCKET_NAME);
    path
}

/// Home directory for the real uid, from the passwd database.
pub fn home_dir() -> Result<PathBuf, SocketError> {
    #[cfg(unix)]
    {
        unix::passwd_home()
    }
    #[cfg(not(unix))]
    {
        Err(SocketError::Unsupported)
    }
}

/// Check `run` and return the socket path.
///
/// [`SocketRole::Listener`] unlinks a socket we own. [`SocketRole::Client`]
/// does not. A regular file is never removed.
pub fn prepare(home: &Path, role: SocketRole) -> Result<PathBuf, SocketError> {
    #[cfg(unix)]
    {
        unix::prepare(home, role)
    }
    #[cfg(not(unix))]
    {
        let _ = (home, role);
        Err(SocketError::Unsupported)
    }
}

#[cfg(unix)]
mod unix {
    use super::{PARTS, SocketError, SocketRole, socket_path};
    use std::ffi::{CStr, c_char};
    use std::fs::{self, DirBuilder};
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    const RUN_MODE: u32 = 0o700;

    pub(super) fn prepare(home: &Path, role: SocketRole) -> Result<PathBuf, SocketError> {
        let sock = socket_path(home);
        ensure_run_dir(home)?;
        check_socket_file(&sock, role)?;
        Ok(sock)
    }

    pub(super) fn passwd_home() -> Result<PathBuf, SocketError> {
        let uid = unsafe { getuid() };
        let mut buf = vec![0u8; 4096];
        loop {
            let mut pwd = std::mem::MaybeUninit::<Passwd>::zeroed();
            let mut result: *mut Passwd = std::ptr::null_mut();
            let rc = unsafe {
                getpwuid_r(
                    uid,
                    pwd.as_mut_ptr(),
                    buf.as_mut_ptr() as *mut c_char,
                    buf.len(),
                    &mut result,
                )
            };
            if rc == ERANGE {
                if buf.len() >= 1024 * 1024 {
                    return Err(SocketError::NoHome);
                }
                buf.resize(buf.len() * 2, 0);
                continue;
            }
            if rc != 0 || result.is_null() {
                return Err(SocketError::NoHome);
            }
            let pwd = unsafe { pwd.assume_init() };
            if pwd.pw_dir.is_null() {
                return Err(SocketError::NoHome);
            }
            let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
            let dir = dir.to_str().map_err(|_| SocketError::NoHome)?;
            if dir.is_empty() {
                return Err(SocketError::NoHome);
            }
            return Ok(PathBuf::from(dir));
        }
    }

    fn ensure_run_dir(home: &Path) -> Result<(), SocketError> {
        let mut current = home.to_path_buf();
        for part in PARTS {
            current.push(part);
            let is_run = part == "run";
            match fs::symlink_metadata(&current) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        return Err(SocketError::Symlink { path: current });
                    }
                    if !meta.is_dir() {
                        return Err(SocketError::NotDirectory { path: current });
                    }
                    if is_run {
                        check_run_owner_and_mode(&current, &meta)?;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    create_dir(&current, is_run)?;
                    let meta = fs::symlink_metadata(&current).map_err(|error| SocketError::Io {
                        path: current.clone(),
                        error,
                    })?;
                    if meta.file_type().is_symlink() || !meta.is_dir() {
                        return Err(SocketError::NotDirectory { path: current });
                    }
                    if is_run {
                        set_mode(&current, RUN_MODE)?;
                        let meta =
                            fs::symlink_metadata(&current).map_err(|error| SocketError::Io {
                                path: current.clone(),
                                error,
                            })?;
                        check_run_owner_and_mode(&current, &meta)?;
                    }
                }
                Err(error) => {
                    return Err(SocketError::Io {
                        path: current,
                        error,
                    });
                }
            }
        }
        Ok(())
    }

    fn create_dir(path: &Path, restricted: bool) -> Result<(), SocketError> {
        let mut builder = DirBuilder::new();
        if restricted {
            builder.mode(RUN_MODE);
        }
        builder.create(path).map_err(|error| SocketError::Io {
            path: path.to_path_buf(),
            error,
        })
    }

    fn set_mode(path: &Path, mode: u32) -> Result<(), SocketError> {
        let mut perms = fs::symlink_metadata(path)
            .map_err(|error| SocketError::Io {
                path: path.to_path_buf(),
                error,
            })?
            .permissions();
        perms.set_mode(mode);
        fs::set_permissions(path, perms).map_err(|error| SocketError::Io {
            path: path.to_path_buf(),
            error,
        })
    }

    fn check_run_owner_and_mode(path: &Path, meta: &std::fs::Metadata) -> Result<(), SocketError> {
        let mode = meta.mode() & 0o777;
        if mode != RUN_MODE {
            return Err(SocketError::WrongMode {
                path: path.to_path_buf(),
                mode,
            });
        }
        if meta.uid() != effective_uid() {
            return Err(SocketError::WrongOwner {
                path: path.to_path_buf(),
            });
        }
        Ok(())
    }

    fn check_socket_file(sock: &Path, role: SocketRole) -> Result<(), SocketError> {
        let meta = match fs::symlink_metadata(sock) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(SocketError::Io {
                    path: sock.to_path_buf(),
                    error,
                });
            }
        };
        let kind = meta.file_type();
        if kind.is_symlink() {
            return Err(SocketError::Symlink {
                path: sock.to_path_buf(),
            });
        }
        if kind.is_socket() {
            if meta.uid() != effective_uid() {
                return Err(SocketError::ForeignSocket {
                    path: sock.to_path_buf(),
                });
            }
            if role == SocketRole::Listener {
                fs::remove_file(sock).map_err(|error| SocketError::Io {
                    path: sock.to_path_buf(),
                    error,
                })?;
            }
            return Ok(());
        }
        if kind.is_file() {
            return Err(SocketError::RegularFile {
                path: sock.to_path_buf(),
            });
        }
        Err(SocketError::NotDirectory {
            path: sock.to_path_buf(),
        })
    }

    fn effective_uid() -> u32 {
        unsafe { geteuid() }
    }

    const ERANGE: i32 = 34;

    /// `getpwuid_r` fills every field. Rust only reads `pw_dir`.
    #[cfg(target_os = "macos")]
    #[allow(dead_code)]
    #[repr(C)]
    struct Passwd {
        pw_name: *mut c_char,
        pw_passwd: *mut c_char,
        pw_uid: u32,
        pw_gid: u32,
        pw_change: i64,
        pw_class: *mut c_char,
        pw_gecos: *mut c_char,
        pw_dir: *mut c_char,
        pw_shell: *mut c_char,
        pw_expire: i64,
        pw_fields: i32,
    }

    /// `getpwuid_r` fills every field. Rust only reads `pw_dir`.
    #[cfg(not(target_os = "macos"))]
    #[allow(dead_code)]
    #[repr(C)]
    struct Passwd {
        pw_name: *mut c_char,
        pw_passwd: *mut c_char,
        pw_uid: u32,
        pw_gid: u32,
        pw_gecos: *mut c_char,
        pw_dir: *mut c_char,
        pw_shell: *mut c_char,
    }

    unsafe extern "C" {
        fn getuid() -> u32;
        fn geteuid() -> u32;
        fn getpwuid_r(
            uid: u32,
            pwd: *mut Passwd,
            buf: *mut c_char,
            buflen: usize,
            result: *mut *mut Passwd,
        ) -> i32;
    }
}
