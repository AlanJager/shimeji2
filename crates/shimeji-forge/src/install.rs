//! Ask the running app to import a pack. Forge does not copy files.
//!
//! The socket is used only when it already exists. [`shimeji_proto::prepare`]
//! creates `Library/Application Support/shimeji2/run`, so this module never
//! calls it. With no listener, the drag-in path is printed and the exit code
//! is 0.

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use shimeji_proto::{InstallRequest, clean_session, encode_install, socket_path};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(100);

/// What `forge install` should print and return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub exit: u8,
    pub message: String,
    pub sent: bool,
}

/// Send an install request for `pack`, using `home` only to find an existing socket.
pub fn install_with_home(pack: &Path, home: &Path) -> InstallReport {
    let path = match pack.canonicalize() {
        Ok(path) => path,
        Err(err) => {
            return InstallReport {
                exit: 2,
                message: format!("cannot read {}: {err}", pack.display()),
                sent: false,
            };
        }
    };
    let path_text = path.display().to_string();
    let name = pack_name(&path);
    let request = InstallRequest {
        v: shimeji_proto::VERSION,
        name,
        path: path_text.clone(),
    };
    let line = match encode_install(&request) {
        Ok(line) => line,
        Err(_) => {
            return InstallReport {
                exit: 2,
                message: format!("cannot send an install request for {path_text}"),
                sent: false,
            };
        }
    };
    if !socket_is_listening(home) {
        return drag(path_text);
    }
    match send_line(&socket_path(home), &line, CONNECT_TIMEOUT) {
        Ok(()) => InstallReport {
            exit: 0,
            message: format!(
                "Asked the Shimeji app to install {} from {path_text}",
                request.name
            ),
            sent: true,
        },
        Err(_) => drag(path_text),
    }
}

fn drag(path: String) -> InstallReport {
    InstallReport {
        exit: 0,
        message: format!("No Shimeji app is listening. Drag this folder in to install:\n{path}"),
        sent: false,
    }
}

fn pack_name(pack: &Path) -> String {
    let raw = pack
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pack");
    let cleaned = clean_session(raw);
    if cleaned.is_empty() {
        "pack".to_string()
    } else {
        cleaned
    }
}

fn socket_is_listening(home: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        match std::fs::symlink_metadata(socket_path(home)) {
            Ok(meta) => meta.file_type().is_socket(),
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = home;
        false
    }
}

fn send_line(path: &Path, line: &str, timeout: Duration) -> io::Result<()> {
    #[cfg(unix)]
    {
        unix::send_line(path, line, timeout)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, line, timeout);
        Err(io::Error::new(io::ErrorKind::Unsupported, "unix socket"))
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::sync::mpsc;
    use std::thread;

    pub(super) fn send_line(path: &Path, line: &str, timeout: Duration) -> io::Result<()> {
        let path = path.to_path_buf();
        let line = line.to_string();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = connect_and_write(&path, &line);
            let _ = tx.send(result);
        });
        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "socket connect timed out",
            )),
        }
    }

    fn connect_and_write(path: &Path, line: &str) -> io::Result<()> {
        let mut stream = UnixStream::connect(path)?;
        stream.write_all(line.as_bytes())?;
        stream.flush()
    }
}
