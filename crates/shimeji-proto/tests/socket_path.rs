//! Socket directory checks. These never call `prepare` on the real home.

use std::fs;
use std::path::{Path, PathBuf};

use shimeji_proto::{SocketError, SocketRole, home_dir, prepare, socket_path};

fn scratch(_label: &str) -> PathBuf {
    // `sun_path` is 104 bytes. The socket suffix is long, so the home stays short.
    let dir = PathBuf::from(format!("/tmp/u14s-{}-{}", std::process::id(), unique()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

#[test]
fn socket_path_is_under_the_uid_home_not_tmpdir() {
    let home = home_dir().expect("passwd home");
    let sock = socket_path(&home);
    assert_eq!(
        sock,
        home.join("Library")
            .join("Application Support")
            .join("shimeji2")
            .join("run")
            .join("sock")
    );
    assert_ne!(home, std::env::temp_dir());
    if let Some(tmpdir) = std::env::var_os("TMPDIR") {
        assert!(
            !sock.starts_with(&tmpdir),
            "socket path {} uses TMPDIR {}",
            sock.display(),
            Path::new(&tmpdir).display()
        );
    }
    let supplied = Path::new("/Users/example");
    assert_eq!(
        socket_path(supplied),
        PathBuf::from("/Users/example/Library/Application Support/shimeji2/run/sock")
    );
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::UnixListener;

    fn mode_of(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    fn set_mode(path: &Path, mode: u32) {
        let mut perms = fs::symlink_metadata(path).unwrap().permissions();
        perms.set_mode(mode);
        fs::set_permissions(path, perms).unwrap();
    }

    #[test]
    fn mode_0755_or_a_symlink_refuses_and_leaves_the_directory() {
        let home = scratch("mode");
        let run = home.join("Library/Application Support/shimeji2/run");
        fs::create_dir_all(&run).unwrap();
        set_mode(&run, 0o755);
        let err = prepare(&home, SocketRole::Listener).unwrap_err();
        assert_eq!(
            err,
            SocketError::WrongMode {
                path: run.clone(),
                mode: 0o755,
            }
        );
        assert_eq!(mode_of(&run), 0o755);

        let home = scratch("link");
        let parent = home.join("Library/Application Support/shimeji2");
        fs::create_dir_all(&parent).unwrap();
        let elsewhere = scratch("link-target");
        set_mode(&elsewhere, 0o700);
        std::os::unix::fs::symlink(&elsewhere, parent.join("run")).unwrap();
        let err = prepare(&home, SocketRole::Listener).unwrap_err();
        assert!(matches!(err, SocketError::Symlink { .. }));
        assert_eq!(mode_of(&elsewhere), 0o700);
        assert!(
            parent
                .join("run")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_stale_socket_is_replaced_and_a_client_leaves_it() {
        let home = scratch("stale");
        let sock = prepare(&home, SocketRole::Listener).unwrap();
        assert_eq!(
            mode_of(sock.parent().unwrap()),
            0o700,
            "run is created mode 0700"
        );
        assert!(!sock.exists());
        let _listener = UnixListener::bind(&sock).unwrap();
        drop(_listener);
        assert!(fs::symlink_metadata(&sock).unwrap().file_type().is_socket());

        prepare(&home, SocketRole::Client).unwrap();
        assert!(
            fs::symlink_metadata(&sock).unwrap().file_type().is_socket(),
            "the hook leaves a live socket in place"
        );

        prepare(&home, SocketRole::Listener).unwrap();
        assert!(!sock.exists(), "the listener unlinks a socket it owns");
        let _again = UnixListener::bind(&sock).unwrap();
    }

    #[test]
    fn a_regular_file_at_the_socket_path_is_kept() {
        let home = scratch("file");
        let sock = prepare(&home, SocketRole::Listener).unwrap();
        fs::write(&sock, b"keep-me").unwrap();
        let err = prepare(&home, SocketRole::Listener).unwrap_err();
        assert_eq!(err, SocketError::RegularFile { path: sock.clone() });
        assert_eq!(fs::read(&sock).unwrap(), b"keep-me");
    }

    #[test]
    fn a_symlink_at_the_socket_path_is_not_removed() {
        let home = scratch("sock-link");
        let sock = prepare(&home, SocketRole::Listener).unwrap();
        let real = home.join("not-the-socket");
        fs::write(&real, b"keep-me").unwrap();
        std::os::unix::fs::symlink(&real, &sock).unwrap();
        let err = prepare(&home, SocketRole::Listener).unwrap_err();
        assert!(matches!(err, SocketError::Symlink { path } if path == sock));
        assert!(
            fs::symlink_metadata(&sock)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&real).unwrap(), b"keep-me");
    }
}
