//! Launch, permission, and quit for one profile directory.
//!
//! The handle and its event queue always exist. The AX worker and the journal
//! thread start only when the phase first reaches [`Phase::On`], and they stop
//! when the user turns mischief off. Off from launch to quit means no thread,
//! no Accessibility call, and no journal file. The clock is not running during
//! the launch wait. A grant that arrives later does not wait on the frame.

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use shimeji_app::journal::{Bounds, LaunchId};
use shimeji_app::mischief_state::{Input, Phase};
use shimeji_app::window_rules::WindowRules;

use super::ax::MacAx;
use super::ax_worker::TargetView;
use super::controller::{JournalClient, Service, WindowHandle};
use super::permission::{self, AxTrust, ProcessTrust, phase_label};
use crate::threads::journal_writer::JournalWriter;

const LAUNCH_WAIT: Duration = Duration::from_secs(2);

/// Ad-hoc signing (the Unit 12 choice) makes macOS treat each rebuild as a new app.
const ADHOC_NOTE: &str = "mischief_note=ad-hoc signing treats each rebuild as a new app; re-tick Accessibility after a rebuild";

pub struct Session {
    phase: Phase,
    noted: bool,
    last_probe: Option<Instant>,
    launch: LaunchId,
    dir: PathBuf,
    handle: WindowHandle,
    /// Held until the worker starts. `None` after the worker has taken it.
    rx: Option<std::sync::mpsc::Receiver<super::ax_worker::Cmd>>,
    service: Option<Service>,
    writer: Option<JournalWriter>,
    shut: bool,
}

impl Session {
    /// `enabled` is the saved switch after the launch env override. The caller
    /// does not write that override back.
    pub fn boot(dir: PathBuf, enabled: bool, rules: WindowRules, screens: Vec<Bounds>) -> Self {
        println!("mischief={}", phase_label(Phase::Off));
        let launch = fresh_launch_id();
        let (handle, rx) = WindowHandle::pair();
        handle.publish(TargetView {
            pid: 0,
            window: 0,
            bounds: Bounds {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            screens,
            rules,
            titles: false,
        });
        let mut session = Self {
            phase: Phase::Off,
            noted: false,
            last_probe: None,
            launch,
            dir,
            handle,
            rx: Some(rx),
            service: None,
            writer: None,
            shut: false,
        };
        if enabled {
            session.turn_on(true);
        }
        session
    }

    pub fn handle(&self) -> WindowHandle {
        self.handle.clone()
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The AX worker or the journal thread is running.
    pub fn worker_running(&self) -> bool {
        self.service.is_some() || self.writer.is_some()
    }

    /// Turn mischief on or off. On feeds [`Input::UserEnabled`] and reads the
    /// grant once. Off feeds [`Input::UserDisabled`], ends a carry, restores,
    /// and stops the worker.
    pub fn set_enabled(&mut self, on: bool) {
        if on {
            self.turn_on(false);
        } else if self.phase != Phase::Off {
            self.feed(Input::UserDisabled, false);
        }
    }

    /// The user closed the awaiting-permission explanation.
    pub fn dismiss_explanation(&mut self) {
        self.feed(Input::ExplanationDismissed, false);
    }

    pub fn on_clock(&mut self, clock_on: bool) {
        let since = self
            .last_probe
            .map(|at| at.elapsed())
            .unwrap_or(permission::TRUST_INTERVAL);
        if !permission::poll_due(self.phase, clock_on, since) {
            return;
        }
        self.last_probe = Some(Instant::now());
        self.feed(Input::Trust(AxTrust.is_trusted()), false);
    }

    pub fn on_activate(&mut self) {
        if !permission::trust_on_activate(self.phase) {
            return;
        }
        self.last_probe = Some(Instant::now());
        self.feed(Input::Trust(AxTrust.is_trusted()), false);
    }

    pub fn publish(
        &self,
        pid: i32,
        window: u64,
        bounds: Bounds,
        screens: Vec<Bounds>,
        rules: WindowRules,
    ) {
        self.handle.publish(TargetView {
            pid,
            window,
            bounds,
            screens,
            rules,
            titles: self.phase == Phase::On,
        });
    }

    pub fn drain(&self) -> Vec<shimeji_core::window_control::WindowEvent> {
        self.handle.drain_events()
    }

    pub fn note_activity(&self, ids: &[u64]) {
        self.handle.note_activity(ids);
    }

    pub fn shutdown(&mut self) {
        if self.shut {
            return;
        }
        self.shut = true;
        if self.phase == Phase::On {
            self.handle.end_carries();
        }
        self.stop_worker();
    }

    fn turn_on(&mut self, wait: bool) {
        if self.phase != Phase::Off {
            return;
        }
        self.feed(Input::UserEnabled, false);
        self.last_probe = Some(Instant::now());
        self.feed(Input::Trust(AxTrust.is_trusted()), wait);
    }

    fn feed(&mut self, input: Input, wait: bool) {
        let step = self.phase.step(input);
        if step.phase != self.phase {
            println!("mischief={}", phase_label(step.phase));
        }
        if step.phase == Phase::AwaitingPermission && !self.noted {
            self.noted = true;
            println!("{ADHOC_NOTE}");
        }
        if self.phase == Phase::Off && step.phase != Phase::Off {
            self.handle.set_engaged(true);
        }
        if step.end_carry {
            self.handle.end_carries();
        }
        let became_on = self.phase != Phase::On && step.phase == Phase::On;
        let became_off = step.phase == Phase::Off && self.phase != Phase::Off;
        self.phase = step.phase;
        self.handle.set_titles(self.phase == Phase::On);
        if became_on {
            self.ensure_worker(wait);
        }
        if became_off {
            self.handle.set_engaged(false);
            self.stop_worker();
        }
    }

    fn ensure_worker(&mut self, wait: bool) {
        if self.service.is_some() {
            self.handle.launch(self.launch);
            if wait {
                let _ = self.handle.wait_booted(LAUNCH_WAIT);
            }
            return;
        }
        let rx = self.rx.take().unwrap_or_else(|| self.handle.reopen());
        let writer = JournalWriter::start(self.dir.clone());
        let client = JournalClient::new(writer.sender());
        let service = Service::start_with(
            rx,
            &self.handle,
            |_stop| MacAx::new(),
            client,
            self.dir.clone(),
        );
        self.writer = Some(writer);
        self.handle.launch(self.launch);
        self.service = Some(service);
        if wait {
            let _ = self.handle.wait_booted(LAUNCH_WAIT);
        }
    }

    fn stop_worker(&mut self) {
        if let Some(service) = self.service.take() {
            service.shutdown(LAUNCH_WAIT);
        }
        if let Some(writer) = self.writer.take() {
            writer.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.shut {
            self.shutdown();
        }
    }
}

fn fresh_launch_id() -> LaunchId {
    let mut bytes = [0u8; 16];
    if let Ok(mut file) = File::open("/dev/urandom") {
        if file.read_exact(&mut bytes).is_ok() {
            return LaunchId::from_bytes(bytes);
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = now.as_nanos().to_le_bytes();
    bytes[..8].copy_from_slice(&nanos[..8]);
    bytes[8..12].copy_from_slice(&std::process::id().to_le_bytes());
    LaunchId::from_bytes(bytes)
}
