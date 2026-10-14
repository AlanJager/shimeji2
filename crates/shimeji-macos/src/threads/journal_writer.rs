//! Journal thread. Appends and fsyncs stay off the frame and off the AX worker.
//!
//! The files are [`shimeji_app::journal::FileLog`] and [`FileSlots`]: mode 0600,
//! append plus fsync for the log, positioned writes and no fsync for slots.
//! Nothing is created until the first header or start, so a mischief-off
//! launch never touches the directory.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use shimeji_app::journal::{
    Bounds, CarryRefusal, EndSpec, FileLog, FileSlots, InstantClock, LaunchId, StartSpec,
    begin_carry, journal_path, slots_path, write_applied, write_end, write_header,
};

/// Work for the journal thread, in order.
pub enum Job {
    Begin {
        spec: StartSpec,
        ack: Sender<Result<(), CarryRefusal>>,
    },
    End(EndSpec),
    Applied {
        slot: u16,
        pid: i32,
        window: u32,
        bounds: Bounds,
    },
    Header(LaunchId),
    /// Drop the open files so a later truncate cannot race this thread's fd.
    Release,
    /// Ack once every job queued before this one has finished.
    Flush(Sender<()>),
    Stop,
}

struct Files {
    log: FileLog,
    slots: FileSlots,
}

impl Files {
    fn open(dir: &Path) -> io::Result<Self> {
        Ok(Self {
            log: FileLog::create(&journal_path(dir))?,
            slots: FileSlots::create(&slots_path(dir))?,
        })
    }
}

/// Owns the thread. Drop sends [`Job::Stop`] and joins.
pub struct JournalWriter {
    tx: Sender<Job>,
    join: Option<JoinHandle<()>>,
}

impl JournalWriter {
    pub fn start(dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("mischief-journal".to_string())
            .spawn(move || run(dir, rx))
            .expect("journal thread");
        Self {
            tx,
            join: Some(join),
        }
    }

    pub fn sender(&self) -> Sender<Job> {
        self.tx.clone()
    }

    pub fn join(mut self) {
        let _ = self.tx.send(Job::Stop);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for JournalWriter {
    fn drop(&mut self) {
        let _ = self.tx.send(Job::Stop);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

fn run(dir: PathBuf, rx: Receiver<Job>) {
    let mut files: Option<Files> = None;
    while let Ok(job) = rx.recv() {
        if !dispatch(&dir, &mut files, job) {
            break;
        }
    }
}

fn dispatch(dir: &Path, files: &mut Option<Files>, job: Job) -> bool {
    match job {
        Job::Stop => false,
        Job::Release => {
            *files = None;
            true
        }
        Job::Flush(ack) => {
            let _ = ack.send(());
            true
        }
        Job::Header(launch) => {
            if let Some(open) = ensure(dir, files) {
                if let Err(error) = write_header(&mut open.log, launch) {
                    eprintln!("mischief_journal={error}");
                }
            }
            true
        }
        Job::Begin { spec, ack } => {
            let result = match ensure(dir, files) {
                Some(open) => begin_carry(&mut open.log, &InstantClock::new(), &spec),
                None => Err(CarryRefusal::WriteFailed),
            };
            let _ = ack.send(result);
            true
        }
        Job::End(spec) => {
            if let Some(open) = ensure(dir, files) {
                if let Err(error) = write_end(&mut open.log, &spec) {
                    eprintln!("mischief_journal={error}");
                }
            }
            true
        }
        Job::Applied {
            slot,
            pid,
            window,
            bounds,
        } => {
            if let Some(open) = ensure(dir, files) {
                if let Err(error) = write_applied(&mut open.slots, slot, pid, window, bounds) {
                    eprintln!("mischief_journal={error}");
                }
            }
            true
        }
    }
}

fn ensure<'a>(dir: &Path, files: &'a mut Option<Files>) -> Option<&'a mut Files> {
    if files.is_none() {
        match Files::open(dir) {
            Ok(open) => *files = Some(open),
            Err(error) => {
                eprintln!("mischief_journal={error}");
                return None;
            }
        }
    }
    files.as_mut()
}

/// Wait until the journal thread has applied every job already queued.
pub fn flush(tx: &Sender<Job>, limit: Duration) -> bool {
    let (ack_tx, ack_rx) = mpsc::channel();
    if tx.send(Job::Flush(ack_tx)).is_err() {
        return false;
    }
    ack_rx.recv_timeout(limit).is_ok()
}
