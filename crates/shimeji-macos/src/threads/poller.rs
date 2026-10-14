//! Window-environment poller. Publishes the latest active window through a
//! single-slot atomic. The main thread never waits on it.

use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::platform::coords::Rect;
use crate::platform::window_env::{ActiveIe, PollPace, full_list, one, pick, to_window_sample};
use shimeji_core::env::WindowSample;

struct SlotCell {
    window: Option<ActiveIe>,
}

/// Latest-wins box. The poller stores; the clock loads and does not free the
/// current cell (the next store does).
pub struct WindowSlot {
    ptr: AtomicPtr<SlotCell>,
}

impl WindowSlot {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            ptr: AtomicPtr::new(ptr::null_mut()),
        })
    }

    pub fn store(&self, window: Option<ActiveIe>) {
        let next = Box::into_raw(Box::new(SlotCell { window }));
        let prev = self.ptr.swap(next, Ordering::AcqRel);
        if !prev.is_null() {
            drop(unsafe { Box::from_raw(prev) });
        }
    }

    pub fn load_sample(&self) -> Option<WindowSample> {
        let ptr = self.ptr.load(Ordering::Acquire);
        if ptr.is_null() {
            return None;
        }
        let cell = unsafe { &*ptr };
        cell.window.as_ref().map(to_window_sample)
    }

    pub fn load_id(&self) -> Option<u64> {
        let ptr = self.ptr.load(Ordering::Acquire);
        if ptr.is_null() {
            return None;
        }
        unsafe { &*ptr }
            .window
            .as_ref()
            .map(|active| active.sample.id)
    }
}

impl Drop for WindowSlot {
    fn drop(&mut self) {
        let ptr = self.ptr.swap(ptr::null_mut(), Ordering::AcqRel);
        if !ptr.is_null() {
            drop(unsafe { Box::from_raw(ptr) });
        }
    }
}

/// Shared knobs the main thread writes and the poller reads.
pub struct PollerCtrl {
    pub stop: AtomicBool,
    pub need_full: AtomicBool,
    pub frontmost_pid: AtomicI32,
    pub pace: AtomicU32,
    pub screens_gen: AtomicU64,
    pub queries: AtomicU64,
    pub query_us: AtomicU64,
    wait: Mutex<()>,
    poke: Condvar,
}

impl PollerCtrl {
    pub fn new(frontmost_pid: i32) -> Arc<Self> {
        Arc::new(Self {
            stop: AtomicBool::new(false),
            need_full: AtomicBool::new(true),
            frontmost_pid: AtomicI32::new(frontmost_pid),
            pace: AtomicU32::new(PollPace::Far as u32),
            screens_gen: AtomicU64::new(1),
            queries: AtomicU64::new(0),
            query_us: AtomicU64::new(0),
            wait: Mutex::new(()),
            poke: Condvar::new(),
        })
    }

    /// Notifies the worker only when the pace value changes. The clock calls
    /// this every tick; waking a Far (500 ms) sleep on every tick would turn
    /// it into a 40 ms poll.
    pub fn set_pace(&self, pace: PollPace) {
        let prev = self.pace.swap(pace as u32, Ordering::AcqRel);
        if prev != pace as u32 {
            self.unpark();
        }
    }

    pub fn query_totals(&self) -> (u64, u64) {
        (
            self.queries.load(Ordering::Relaxed),
            self.query_us.load(Ordering::Relaxed),
        )
    }

    fn unpark(&self) {
        let _guard = lock(&self.wait);
        self.poke.notify_one();
    }
}

pub struct Poller {
    pub slot: Arc<WindowSlot>,
    pub ctrl: Arc<PollerCtrl>,
    screens: Arc<std::sync::Mutex<Vec<Rect>>>,
    join: Option<JoinHandle<()>>,
}

impl Poller {
    pub fn start(our_pid: i32, frontmost_pid: i32, screens: Vec<Rect>) -> Self {
        let slot = WindowSlot::new();
        let ctrl = PollerCtrl::new(frontmost_pid);
        let screens = Arc::new(std::sync::Mutex::new(screens));
        let slot_t = Arc::clone(&slot);
        let ctrl_t = Arc::clone(&ctrl);
        let screens_t = Arc::clone(&screens);
        let join = thread::Builder::new()
            .name("shimeji-window-env".into())
            .spawn(move || run(our_pid, slot_t, ctrl_t, screens_t))
            .expect("spawn window-env poller");
        Self {
            slot,
            ctrl,
            screens,
            join: Some(join),
        }
    }

    pub fn set_screens(&self, screens: Vec<Rect>) {
        if let Ok(mut guard) = self.screens.lock() {
            *guard = screens;
        }
        self.ctrl.screens_gen.fetch_add(1, Ordering::AcqRel);
        self.ctrl.need_full.store(true, Ordering::Release);
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.ctrl.stop.store(true, Ordering::Release);
        self.ctrl.unpark();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn run(
    our_pid: i32,
    slot: Arc<WindowSlot>,
    ctrl: Arc<PollerCtrl>,
    screens: Arc<std::sync::Mutex<Vec<Rect>>>,
) {
    let mut tracked: Option<u64> = None;
    let mut last_full = Instant::now() - Duration::from_secs(2);
    while !ctrl.stop.load(Ordering::Acquire) {
        let pace = pace_from(ctrl.pace.load(Ordering::Acquire));
        if pace == PollPace::Stopped {
            let guard = lock(&ctrl.wait);
            if ctrl.stop.load(Ordering::Acquire) {
                break;
            }
            if pace_from(ctrl.pace.load(Ordering::Acquire)) == PollPace::Stopped {
                let _guard = ctrl.poke.wait(guard).unwrap_or_else(|err| err.into_inner());
            }
            continue;
        }
        let frontmost = ctrl.frontmost_pid.load(Ordering::Acquire);
        let need_full = ctrl.need_full.swap(false, Ordering::AcqRel)
            || tracked.is_none()
            || last_full.elapsed() >= Duration::from_secs(1);
        let screen_rects = screens.lock().map(|g| g.clone()).unwrap_or_default();
        let records = if need_full {
            last_full = Instant::now();
            timed(&ctrl, full_list)
        } else if let Some(id) = tracked {
            let one_list = timed(&ctrl, || one(id));
            let still = pick(&one_list, frontmost, our_pid, &screen_rects)
                .is_some_and(|active| active.sample.id == id);
            if still {
                one_list
            } else {
                last_full = Instant::now();
                timed(&ctrl, full_list)
            }
        } else {
            last_full = Instant::now();
            timed(&ctrl, full_list)
        };
        let active = pick(&records, frontmost, our_pid, &screen_rects);
        tracked = active.as_ref().map(|item| item.sample.id);
        slot.store(active);
        let guard = lock(&ctrl.wait);
        if ctrl.stop.load(Ordering::Acquire) {
            break;
        }
        let pace = pace_from(ctrl.pace.load(Ordering::Acquire));
        let Some(wait_ms) = wait_ms(pace) else {
            continue;
        };
        let _guard = ctrl
            .poke
            .wait_timeout(guard, Duration::from_millis(wait_ms))
            .unwrap_or_else(|err| err.into_inner());
    }
}

fn lock(mutex: &Mutex<()>) -> MutexGuard<'_, ()> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn timed<T>(ctrl: &PollerCtrl, body: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = body();
    let us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    ctrl.queries.fetch_add(1, Ordering::Relaxed);
    ctrl.query_us.fetch_add(us, Ordering::Relaxed);
    value
}

fn pace_from(value: u32) -> PollPace {
    match value {
        0 => PollPace::Stopped,
        1 => PollPace::Ride,
        2 => PollPace::Near,
        _ => PollPace::Far,
    }
}

fn wait_ms(pace: PollPace) -> Option<u64> {
    match pace {
        PollPace::Stopped => None,
        PollPace::Ride => Some(40),
        PollPace::Near => Some(100),
        PollPace::Far => Some(500),
    }
}
