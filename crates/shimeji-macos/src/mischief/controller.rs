//! `WindowControl` for the shell. `request_move` only enqueues.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use shimeji_app::journal::{Bounds, CarryRefusal, EndSpec, LaunchId, StartSpec};
use shimeji_core::window_control::{WindowControl, WindowEvent, WindowMove};

use super::ax_worker::{
    self, AxBackend, Cmd, Flight, JournalPort, PendingMove, Shared, TargetView, lock,
};
use crate::threads::journal_writer::{self, Job};

/// Cloneable handle the runtime keeps.
///
/// The queue always exists once [`WindowHandle::pair`] has been called.
/// `engaged` is set when the user turns mischief on. Until then
/// [`WindowControl::request_move`] is silent. After that, a handle that is
/// not live fails the move so the pet falls. `None` inside is the inert
/// handle: requests are ignored and no event is produced.
#[derive(Clone, Default)]
pub struct WindowHandle {
    inner: Option<Arc<HandleInner>>,
}

struct HandleInner {
    tx: Mutex<Sender<Cmd>>,
    events: Arc<Mutex<Vec<WindowEvent>>>,
    flight: Arc<Flight>,
    /// The user has asked for mischief. Off stays silent, like no window control.
    engaged: AtomicBool,
    live: Arc<AtomicBool>,
    target: Arc<Mutex<TargetView>>,
    booted: Arc<AtomicBool>,
    open: Mutex<HashSet<u64>>,
}

/// The AX worker. Drop stops it and does not restore. [`Service::shutdown`]
/// restores, then stops.
pub struct Service {
    tx: Sender<Cmd>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl WindowHandle {
    pub fn pair() -> (Self, Receiver<Cmd>) {
        let (tx, rx) = mpsc::channel();
        let inner = Arc::new(HandleInner {
            tx: Mutex::new(tx),
            events: Arc::new(Mutex::new(Vec::new())),
            flight: Arc::new(Flight::new()),
            engaged: AtomicBool::new(false),
            live: Arc::new(AtomicBool::new(false)),
            target: Arc::new(Mutex::new(TargetView::default())),
            booted: Arc::new(AtomicBool::new(false)),
            open: Mutex::new(HashSet::new()),
        });
        (Self { inner: Some(inner) }, rx)
    }

    pub fn publish(&self, view: TargetView) {
        let Some(inner) = &self.inner else {
            return;
        };
        *lock(&inner.target) = view;
    }

    pub(crate) fn snapshot(&self) -> TargetView {
        match &self.inner {
            Some(inner) => lock(&inner.target).clone(),
            None => TargetView::default(),
        }
    }

    /// The user turned mischief on or off. Off ignores moves. On, a handle
    /// that is not yet live fails them.
    pub fn set_engaged(&self, on: bool) {
        if let Some(inner) = &self.inner {
            inner.engaged.store(on, Ordering::Release);
        }
    }

    pub(crate) fn set_titles(&self, on: bool) {
        if let Some(inner) = &self.inner {
            lock(&inner.target).titles = on;
        }
    }

    /// A fresh command channel after the worker has dropped the previous one.
    /// The boot flag goes back to false so the next launch can be waited on.
    pub(crate) fn reopen(&self) -> Receiver<Cmd> {
        let (tx, rx) = mpsc::channel();
        if let Some(inner) = &self.inner {
            *lock(&inner.tx) = tx;
            inner.booted.store(false, Ordering::Release);
        }
        rx
    }

    pub fn set_live(&self, on: bool) {
        if let Some(inner) = &self.inner {
            inner.live.store(on, Ordering::Release);
        }
    }

    pub fn launch(&self, launch: LaunchId) {
        self.send(Cmd::LaunchRestore(launch));
    }

    fn send(&self, cmd: Cmd) {
        let Some(inner) = &self.inner else {
            return;
        };
        let tx = lock(&inner.tx).clone();
        let _ = tx.send(cmd);
    }

    pub fn wait_booted(&self, limit: Duration) -> bool {
        let Some(inner) = &self.inner else {
            return true;
        };
        let start = std::time::Instant::now();
        while !inner.booted.load(Ordering::Acquire) {
            if start.elapsed() >= limit {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
        true
    }

    /// Fail every window this handle has asked to move, and tell the worker
    /// to end those carries and restore them.
    pub fn end_carries(&self) {
        let Some(inner) = &self.inner else {
            return;
        };
        inner.live.store(false, Ordering::Release);
        let ids: Vec<u64> = lock(&inner.open).drain().collect();
        {
            let mut events = lock(&inner.events);
            for id in ids {
                events.push(WindowEvent::Failed { id });
            }
        }
        self.send(Cmd::EndAll);
    }

    /// Window ids held by live carry actions, plus requests still emitted this frame.
    pub fn note_activity(&self, ids: &[u64]) {
        self.send(Cmd::Activity(ids.to_vec()));
    }

    pub fn drain_events(&self) -> Vec<WindowEvent> {
        let Some(inner) = &self.inner else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(id) = inner.flight.timed_out(ax_worker::MOVE_TIMEOUT) {
            out.push(WindowEvent::Failed { id });
            self.send(Cmd::TimedOut(id));
        }
        out.extend(lock(&inner.events).drain(..));
        out
    }

    fn fail(&self, id: u64) {
        let Some(inner) = &self.inner else {
            return;
        };
        lock(&inner.events).push(WindowEvent::Failed { id });
    }
}

impl WindowControl for WindowHandle {
    fn request_move(&mut self, request: &WindowMove) {
        let Some(inner) = &self.inner else {
            return;
        };
        if !inner.live.load(Ordering::Acquire) {
            if inner.engaged.load(Ordering::Acquire) {
                self.fail(request.id);
            }
            return;
        }
        let target = lock(&inner.target).clone();
        let Ok(window) = u32::try_from(request.id) else {
            self.fail(request.id);
            return;
        };
        if window == 0 || target.pid <= 0 || target.window != request.id {
            self.fail(request.id);
            return;
        }
        lock(&inner.open).insert(request.id);
        let pending = PendingMove {
            id: request.id,
            pid: target.pid,
            window,
            origin: target.bounds,
            dest: Bounds {
                x: request.x,
                y: request.y,
                width: request.width,
                height: request.height,
            },
        };
        self.send(Cmd::Move(pending));
    }
}

impl Service {
    pub fn start_with<F, B, J>(
        rx: Receiver<Cmd>,
        handle: &WindowHandle,
        factory: F,
        journal: J,
        dir: PathBuf,
    ) -> Self
    where
        F: FnOnce(Arc<AtomicBool>) -> B,
        B: AxBackend + 'static,
        J: JournalPort + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let backend = factory(Arc::clone(&stop));
        let inner = handle.inner.as_ref().expect("mischief handle").clone();
        let shared = Shared {
            events: Arc::clone(&inner.events),
            flight: Arc::clone(&inner.flight),
            live: Arc::clone(&inner.live),
            target: Arc::clone(&inner.target),
            booted: Arc::clone(&inner.booted),
            stop: Arc::clone(&stop),
        };
        let tx = lock(&inner.tx).clone();
        let join = thread::Builder::new()
            .name("mischief-ax".to_string())
            .spawn(move || ax_worker::run(rx, backend, journal, shared, dir))
            .expect("ax thread");
        Self {
            tx,
            stop,
            join: Some(join),
        }
    }

    /// End carries, restore, and wait up to `limit` for the worker to finish.
    pub fn shutdown(mut self, limit: Duration) {
        let (ack_tx, ack_rx) = mpsc::channel();
        let _ = self.tx.send(Cmd::Shutdown(ack_tx));
        let _ = ack_rx.recv_timeout(limit);
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.tx.send(Cmd::Stop);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

/// Forwards journal work to [`crate::threads::journal_writer`] and polls the
/// 50 ms gate without blocking the AX worker on the fsync itself.
pub struct JournalClient {
    tx: Sender<Job>,
    pending: HashMap<u64, Receiver<Result<(), CarryRefusal>>>,
}

impl JournalClient {
    pub fn new(tx: Sender<Job>) -> Self {
        Self {
            tx,
            pending: HashMap::new(),
        }
    }
}

impl JournalPort for JournalClient {
    fn request_begin(&mut self, id: u64, spec: StartSpec) {
        let (ack_tx, ack_rx) = mpsc::channel();
        let _ = self.tx.send(Job::Begin { spec, ack: ack_tx });
        self.pending.insert(id, ack_rx);
    }

    fn poll_begin(&mut self, id: u64) -> Option<Result<(), CarryRefusal>> {
        let rx = self.pending.get(&id)?;
        match rx.try_recv() {
            Ok(result) => {
                self.pending.remove(&id);
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending.remove(&id);
                Some(Err(CarryRefusal::WriteFailed))
            }
        }
    }

    fn write_end(&mut self, spec: &EndSpec) {
        let _ = self.tx.send(Job::End(*spec));
    }

    fn write_applied(&mut self, slot: u16, pid: i32, window: u32, bounds: Bounds) {
        let _ = self.tx.send(Job::Applied {
            slot,
            pid,
            window,
            bounds,
        });
    }

    fn write_header(&mut self, launch: LaunchId) {
        let _ = self.tx.send(Job::Header(launch));
    }

    fn release_files(&mut self) {
        let _ = self.tx.send(Job::Release);
    }

    fn flush(&mut self) {
        let _ = journal_writer::flush(&self.tx, Duration::from_millis(1000));
    }
}
