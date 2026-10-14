//! One line on the per-user socket, with a connect timeout.
//!
//! Failure is returned to the caller. The binary still exits 0.

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

/// Connect, write `line`, and close. `timeout` bounds the connect.
pub fn send_line(path: &Path, line: &str, timeout: Duration) -> io::Result<()> {
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
