//! One thread accepts connections. Each connection is read on its own thread
//! so a silent client cannot hold up the next event.

use std::io::{ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use shimeji_proto::{AgentEvent, InstallRequest, MAX_LINE_BYTES};
use shimeji_proto::{Message, ParseOutcome, SocketRole, parse_line, prepare};

/// One line the listener accepted. Install requests are not imported here.
#[derive(Debug, Clone)]
pub enum Inbound {
    Event(AgentEvent),
    Install(InstallRequest),
}

/// How many connections may be reading at once. One past this is closed.
pub const MAX_CONNECTIONS: usize = 8;

/// A client that connects and sends nothing is dropped after this long.
pub const READ_TIMEOUT: Duration = Duration::from_millis(250);

/// Same-uid callers are half-trusted. Any other uid is rejected.
pub fn peer_allowed(peer: u32, ours: u32) -> bool {
    peer == ours
}

/// `None` is a failed `getpeereid`, which is also a rejection.
pub fn admit(peer: Option<u32>, ours: u32) -> bool {
    match peer {
        Some(peer) => peer_allowed(peer, ours),
        None => false,
    }
}

pub fn current_uid() -> u32 {
    unsafe { getuid() }
}

/// Accept loop. Drop sets the stop flag, joins the accept thread, and waits
/// for in-flight reads. The callback is not waited on.
pub struct Listener {
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    accept: Option<JoinHandle<()>>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
        let started = std::time::Instant::now();
        while self.inflight.load(Ordering::Acquire) > 0
            && started.elapsed() < Duration::from_secs(2)
        {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Bind `prepare(home, Listener)`. A failed prepare returns the error and
/// starts no thread. `deliver_on_main` posts the event with `dispatch_async`;
/// tests pass `false` and receive the event on the read thread.
pub fn start<F>(home: &Path, deliver_on_main: bool, on_event: F) -> Result<Listener, String>
where
    F: Fn(Inbound) + Send + Sync + 'static,
{
    let path = prepare(home, SocketRole::Listener).map_err(|error| error.to_string())?;
    let bound = UnixListener::bind(&path).map_err(|error| error.to_string())?;
    bound
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

    let stop = Arc::new(AtomicBool::new(false));
    let inflight = Arc::new(AtomicUsize::new(0));
    let callback = Arc::new(on_event);
    let stop_flag = Arc::clone(&stop);
    let inflight_flag = Arc::clone(&inflight);
    let accept = thread::Builder::new()
        .name("agent-listener".to_string())
        .spawn(move || accept_loop(bound, stop_flag, inflight_flag, callback, deliver_on_main))
        .map_err(|error| error.to_string())?;
    Ok(Listener {
        stop,
        inflight,
        accept: Some(accept),
    })
}

fn accept_loop<F>(
    listener: UnixListener,
    stop: Arc<AtomicBool>,
    inflight: Arc<AtomicUsize>,
    callback: Arc<F>,
    deliver_on_main: bool,
) where
    F: Fn(Inbound) + Send + Sync + 'static,
{
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if inflight.load(Ordering::Acquire) >= MAX_CONNECTIONS {
                    drop(stream);
                    continue;
                }
                inflight.fetch_add(1, Ordering::AcqRel);
                let callback = Arc::clone(&callback);
                let inflight = Arc::clone(&inflight);
                thread::spawn(move || {
                    handle_one(stream, deliver_on_main, &callback);
                    inflight.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn handle_one<F>(stream: UnixStream, deliver_on_main: bool, on_event: &Arc<F>)
where
    F: Fn(Inbound) + Send + Sync + 'static,
{
    if !admit(peer_uid(&stream), current_uid()) {
        return;
    }
    let mut stream = stream;
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let Some(line) = read_one_line(&mut stream) else {
        return;
    };
    let inbound = match parse_line(&line) {
        Ok(ParseOutcome::Message(Message::Event(event))) => Inbound::Event(event),
        Ok(ParseOutcome::Message(Message::Install(request))) => Inbound::Install(request),
        _ => return,
    };
    if deliver_on_main {
        let callback = Arc::clone(on_event);
        crate::threads::wake::on_main(move || callback(inbound));
    } else {
        on_event(inbound);
    }
}

/// One line, at most [`MAX_LINE_BYTES`]. No newline within that cap drops the
/// connection, which is how an 8 KB write is discarded.
fn read_one_line(stream: &mut UnixStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if buf.len() >= MAX_LINE_BYTES {
            return None;
        }
        match stream.read(&mut byte) {
            Ok(0) => return None,
            Ok(_) => {
                buf.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
    String::from_utf8(buf).ok()
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut uid = 0u32;
    let mut gid = 0u32;
    // `fd` is the connected socket. `uid` and `gid` are local and live for the call.
    let rc = unsafe { getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    if rc == 0 { Some(uid) } else { None }
}

unsafe extern "C" {
    fn getuid() -> u32;
    fn getpeereid(fd: i32, uid: *mut u32, gid: *mut u32) -> i32;
}
