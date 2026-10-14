//! Mischief journal and the replay that puts windows back.
//!
//! Two files, both created mode 0600:
//!
//! - `mischief-journal.jsonl` — a header (launch id and version), then one
//!   `start` and one `end` per carry or throw. A start is fsynced before the
//!   first move. Titles are not a field.
//! - `mischief-slots.bin` — fixed-size records, one per carry slot, holding
//!   the last bounds a pet applied. Positioned writes, no fsync: the bytes
//!   stay in the page cache if the process dies.
//!
//! Replay treats both files as untrusted. The app name comes from the live
//! process, never from the file. [`SlotAllocator`] gives each window one slot
//! for the session. Replay puts the window back where it was before the first
//! carry of that session.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::window_rules::{Decision, DenyReason, WindowRules};

/// Append log. One JSON object per line.
pub const JOURNAL_FILE: &str = "mischief-journal.jsonl";

/// Latest applied bounds. One [`SLOT_LEN`]-byte record per slot.
pub const SLOTS_FILE: &str = "mischief-slots.bin";

pub const JOURNAL_VERSION: u32 = 1;

/// A larger file is refused and not read.
pub const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;

/// How many non-empty lines are parsed. The rest are ignored.
pub const MAX_JOURNAL_LINES: usize = 20_000;

/// The start entry's fsync must be acknowledged inside this budget.
pub const START_SYNC_LIMIT: Duration = Duration::from_millis(50);

/// Entries this old, or older, are not restored.
pub const MAX_ENTRY_AGE_MS: u64 = 24 * 60 * 60 * 1000;

/// CG bounds and an AX window match when each edge is at most this far apart.
pub const AX_MATCH_PT: i32 = 1;

pub const AX_STANDARD_WINDOW: &str = "AXStandardWindow";

pub const SLOT_LEN: usize = 64;
pub const SLOT_COUNT: usize = 128;

const SLOT_TAG: &[u8; 4] = b"SLOT";

/// Top-left origin, y down. Sizes are in logical points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 16 bytes the shell chooses once per launch. Serialized as 32 hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LaunchId([u8; 16]);

impl LaunchId {
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(32);
        for byte in self.0 {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
        out
    }

    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 32 {
            return None;
        }
        let bytes = text.as_bytes();
        let mut out = [0u8; 16];
        for i in 0..16 {
            let hi = hex_digit(bytes[i * 2])?;
            let lo = hex_digit(bytes[i * 2 + 1])?;
            out[i] = (hi << 4) | lo;
        }
        Some(Self(out))
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Carry,
    Throw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartSpec {
    pub op: Operation,
    pub pid: i32,
    pub window: u32,
    pub slot: u16,
    pub bounds: Bounds,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndSpec {
    pub op: Operation,
    pub pid: i32,
    pub window: u32,
    pub slot: u16,
    pub at_ms: u64,
}

/// One AX window the shell observed for a process.
///
/// `title` is `None` when Accessibility is not granted, and the rules then
/// ignore title patterns. `subrole` is the AX string (`AXStandardWindow`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxWindow {
    pub bounds: Bounds,
    pub subrole: String,
    pub position_settable: bool,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxMiss {
    NoUniqueWindow,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarryDeny {
    Ax(AxMiss),
    NotSettable,
    Rules(DenyReason),
}

/// CG bounds to the one standard AX window of this process, or a refusal.
///
/// Only `AXStandardWindow` candidates count. Position and size must agree
/// within [`AX_MATCH_PT`]. Zero or two matches refuse the carry.
pub fn match_ax_window(cg: Bounds, windows: &[AxWindow]) -> Result<usize, AxMiss> {
    let mut found = None;
    for (index, window) in windows.iter().enumerate() {
        if window.subrole != AX_STANDARD_WINDOW {
            continue;
        }
        if !within_pt(cg, window.bounds) {
            continue;
        }
        if found.is_some() {
            return Err(AxMiss::Ambiguous);
        }
        found = Some(index);
    }
    found.ok_or(AxMiss::NoUniqueWindow)
}

/// The checks a live carry and a restore share: a unique standard window,
/// a settable position, and the window rules. The title used for the rules
/// is the matched window's title, which is absent without the grant.
pub fn assess_carry(
    rules: &WindowRules,
    app_name: &str,
    cg: Bounds,
    windows: &[AxWindow],
) -> Result<usize, CarryDeny> {
    let index = match_ax_window(cg, windows).map_err(CarryDeny::Ax)?;
    if !windows[index].position_settable {
        return Err(CarryDeny::NotSettable);
    }
    match rules.decision(app_name, windows[index].title.as_deref()) {
        Decision::Allow => Ok(index),
        Decision::Deny(reason) => Err(CarryDeny::Rules(reason)),
    }
}

fn within_pt(cg: Bounds, ax: Bounds) -> bool {
    let slop = i64::from(AX_MATCH_PT);
    edge(cg.x, ax.x) <= slop
        && edge(cg.y, ax.y) <= slop
        && edge(cg.width, ax.width) <= slop
        && edge(cg.height, ax.height) <= slop
}

fn edge(left: i32, right: i32) -> i64 {
    (i64::from(left) - i64::from(right)).abs()
}

/// Move `bounds` the smallest amount that leaves at least one point on a
/// visible screen. A rectangle that already intersects a screen is unchanged.
/// `None` when there is no screen to land on.
pub fn clamp_partly_on_screen(bounds: Bounds, screens: &[Bounds]) -> Option<Bounds> {
    if screens.is_empty() {
        return None;
    }
    if partly_on_screen(bounds, screens) {
        return Some(bounds);
    }
    let screen = screens
        .iter()
        .copied()
        .min_by_key(|screen| gap(bounds, *screen))?;
    Some(pull_onto(bounds, screen))
}

pub fn partly_on_screen(bounds: Bounds, screens: &[Bounds]) -> bool {
    screens.iter().any(|screen| intersects(bounds, *screen))
}

fn intersects(window: Bounds, screen: Bounds) -> bool {
    if window.width <= 0 || window.height <= 0 || screen.width <= 0 || screen.height <= 0 {
        return false;
    }
    let x = i64::from(window.x);
    let y = i64::from(window.y);
    let sx = i64::from(screen.x);
    let sy = i64::from(screen.y);
    x < sx + i64::from(screen.width)
        && x + i64::from(window.width) > sx
        && y < sy + i64::from(screen.height)
        && y + i64::from(window.height) > sy
}

fn gap(window: Bounds, screen: Bounds) -> i64 {
    axis_gap(window.x, window.width, screen.x, screen.width).saturating_add(axis_gap(
        window.y,
        window.height,
        screen.y,
        screen.height,
    ))
}

fn axis_gap(origin: i32, length: i32, screen_origin: i32, screen_length: i32) -> i64 {
    let start = i64::from(origin);
    let end = start + i64::from(length);
    let screen_start = i64::from(screen_origin);
    let screen_end = screen_start + i64::from(screen_length);
    if end <= screen_start {
        screen_start - end
    } else if start >= screen_end {
        start - screen_end
    } else {
        0
    }
}

fn pull_onto(window: Bounds, screen: Bounds) -> Bounds {
    let mut x = i64::from(window.x);
    let mut y = i64::from(window.y);
    let width = i64::from(window.width);
    let height = i64::from(window.height);
    let left = i64::from(screen.x);
    let top = i64::from(screen.y);
    let right = left + i64::from(screen.width);
    let bottom = top + i64::from(screen.height);
    if width > 0 {
        if x >= right {
            x = right - 1;
        } else if x + width <= left {
            x = left - width + 1;
        }
    }
    if height > 0 {
        if y >= bottom {
            y = bottom - 1;
        } else if y + height <= top {
            y = top - height + 1;
        }
    }
    Bounds {
        x: clamp_i32(x),
        y: clamp_i32(y),
        width: window.width,
        height: window.height,
    }
}

fn clamp_i32(value: i64) -> i32 {
    if value > i64::from(i32::MAX) {
        i32::MAX
    } else if value < i64::from(i32::MIN) {
        i32::MIN
    } else {
        value as i32
    }
}

/// Last bounds written into a slot. `seq` starts at 1 and counts writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    pub pid: i32,
    pub window: u32,
    pub bounds: Bounds,
    pub seq: u64,
}

/// Positioned slot I/O. Implementations must not fsync.
pub trait SlotIo {
    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> io::Result<()>;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;
}

/// Append log. `sync_log` is the fsync the carry gate waits for.
pub trait LogIo {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()>;
    fn sync_log(&mut self) -> io::Result<()>;
}

pub trait ElapsedClock {
    fn elapsed(&self) -> Duration;
}

#[derive(Debug, Clone, Copy)]
pub struct InstantClock(Instant);

impl InstantClock {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Default for InstantClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ElapsedClock for InstantClock {
    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}

/// The start line was not durable in time, or the session has no free slot.
/// The window must not move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarryRefusal {
    WriteFailed,
    SyncTimeout,
    /// [`SlotAllocator`] has given every slot to a window already.
    NoFreeSlot,
}

impl CarryRefusal {
    /// The shell leaves the window where it is.
    pub fn window_stays_put(self) -> bool {
        matches!(
            self,
            Self::WriteFailed | Self::SyncTimeout | Self::NoFreeSlot
        )
    }

    /// The pet leaves the carry and falls.
    pub fn pet_falls(self) -> bool {
        matches!(
            self,
            Self::WriteFailed | Self::SyncTimeout | Self::NoFreeSlot
        )
    }
}

/// One slot per `(pid, window)` for a whole session.
///
/// U11b calls [`assign`](Self::assign) before `begin_carry` and stores the
/// index on the start line and in [`write_applied`]. The first carry of a
/// window takes the lowest free slot. Every later carry of that window
/// reuses it. A slot is never given to a different window in the same
/// session. When every slot is taken, `assign` returns
/// [`CarryRefusal::NoFreeSlot`]: the window stays put and the pet falls.
#[derive(Debug, Clone)]
pub struct SlotAllocator {
    owners: [Option<(i32, u32)>; SLOT_COUNT],
}

impl SlotAllocator {
    pub fn new() -> Self {
        Self {
            owners: [None; SLOT_COUNT],
        }
    }

    /// The slot this window keeps until the session ends.
    pub fn assign(&mut self, pid: i32, window: u32) -> Result<u16, CarryRefusal> {
        let key = (pid, window);
        if let Some(index) = self.owners.iter().position(|owner| *owner == Some(key)) {
            return Ok(index as u16);
        }
        let Some(index) = self.owners.iter().position(|owner| owner.is_none()) else {
            return Err(CarryRefusal::NoFreeSlot);
        };
        self.owners[index] = Some(key);
        Ok(index as u16)
    }
}

impl Default for SlotAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Append `line`, fsync, and refuse when that takes longer than `limit`
/// or the write fails. `line` gains a trailing newline if it lacks one.
pub fn authorize_start<L: LogIo, C: ElapsedClock>(
    log: &mut L,
    clock: &C,
    line: &[u8],
    limit: Duration,
) -> Result<(), CarryRefusal> {
    let started = clock.elapsed();
    let wrote = append_synced(log, line);
    let took = clock
        .elapsed()
        .checked_sub(started)
        .unwrap_or(Duration::ZERO);
    match wrote {
        Err(err) if err.kind() == ErrorKind::TimedOut => Err(CarryRefusal::SyncTimeout),
        Err(_) => Err(CarryRefusal::WriteFailed),
        Ok(()) if took > limit => Err(CarryRefusal::SyncTimeout),
        Ok(()) => Ok(()),
    }
}

fn append_synced<L: LogIo>(log: &mut L, line: &[u8]) -> io::Result<()> {
    log.append(line)?;
    if !line.ends_with(b"\n") {
        log.append(b"\n")?;
    }
    log.sync_log()
}

pub fn write_header<L: LogIo>(log: &mut L, launch: LaunchId) -> io::Result<()> {
    let bytes = serde_json::to_vec(&Line::Header {
        v: JOURNAL_VERSION,
        launch: launch.to_hex(),
    })
    .map_err(invalid_json)?;
    append_synced(log, &bytes)
}

pub fn begin_carry<L: LogIo, C: ElapsedClock>(
    log: &mut L,
    clock: &C,
    spec: &StartSpec,
) -> Result<(), CarryRefusal> {
    let bytes =
        serde_json::to_vec(&Line::from_start(spec)).map_err(|_| CarryRefusal::WriteFailed)?;
    authorize_start(log, clock, &bytes, START_SYNC_LIMIT)
}

pub fn write_end<L: LogIo>(log: &mut L, spec: &EndSpec) -> io::Result<()> {
    let bytes = serde_json::to_vec(&Line::from_end(spec)).map_err(invalid_json)?;
    append_synced(log, &bytes)
}

fn invalid_json(err: serde_json::Error) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, err)
}

/// Write the bounds a pet just applied. Does not fsync.
pub fn write_applied<S: SlotIo>(
    slots: &mut S,
    index: u16,
    pid: i32,
    window: u32,
    bounds: Bounds,
) -> io::Result<()> {
    if index as usize >= SLOT_COUNT {
        return Err(io::Error::new(ErrorKind::InvalidInput, "slot index"));
    }
    let seq = read_applied(slots, index)?
        .map(|applied| applied.seq.saturating_add(1))
        .unwrap_or(1)
        .max(1);
    let bytes = encode_slot(&Applied {
        pid,
        window,
        bounds,
        seq,
    });
    slots.write_at(u64::from(index) * SLOT_LEN as u64, &bytes)
}

pub fn read_applied<S: SlotIo>(slots: &mut S, index: u16) -> io::Result<Option<Applied>> {
    if index as usize >= SLOT_COUNT {
        return Ok(None);
    }
    let mut buf = [0u8; SLOT_LEN];
    let n = slots.read_at(u64::from(index) * SLOT_LEN as u64, &mut buf)?;
    if n < SLOT_LEN {
        return Ok(None);
    }
    Ok(decode_slot(&buf))
}

fn encode_slot(applied: &Applied) -> [u8; SLOT_LEN] {
    let mut buf = [0u8; SLOT_LEN];
    buf[0..4].copy_from_slice(SLOT_TAG);
    buf[4..8].copy_from_slice(&applied.pid.to_le_bytes());
    buf[8..12].copy_from_slice(&applied.window.to_le_bytes());
    buf[12..16].copy_from_slice(&applied.bounds.x.to_le_bytes());
    buf[16..20].copy_from_slice(&applied.bounds.y.to_le_bytes());
    buf[20..24].copy_from_slice(&applied.bounds.width.to_le_bytes());
    buf[24..28].copy_from_slice(&applied.bounds.height.to_le_bytes());
    buf[28..36].copy_from_slice(&applied.seq.to_le_bytes());
    let sum = checksum(&buf[..60]);
    buf[60..64].copy_from_slice(&sum.to_le_bytes());
    buf
}

fn decode_slot(buf: &[u8]) -> Option<Applied> {
    if buf.len() < SLOT_LEN || buf[0..4] != SLOT_TAG[..] {
        return None;
    }
    let sum = u32::from_le_bytes(buf[60..64].try_into().ok()?);
    if sum != checksum(&buf[..60]) {
        return None;
    }
    let seq = u64::from_le_bytes(buf[28..36].try_into().ok()?);
    if seq == 0 {
        return None;
    }
    Some(Applied {
        pid: i32::from_le_bytes(buf[4..8].try_into().ok()?),
        window: u32::from_le_bytes(buf[8..12].try_into().ok()?),
        bounds: Bounds {
            x: i32::from_le_bytes(buf[12..16].try_into().ok()?),
            y: i32::from_le_bytes(buf[16..20].try_into().ok()?),
            width: i32::from_le_bytes(buf[20..24].try_into().ok()?),
            height: i32::from_le_bytes(buf[24..28].try_into().ok()?),
        },
        seq,
    })
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut sum = 0x811c_9dc5u32;
    for byte in bytes {
        sum ^= u32::from(*byte);
        sum = sum.wrapping_mul(0x0100_0193);
    }
    sum
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedJournal {
    pub last_launch: Option<LaunchId>,
    pub starts: Vec<ParsedStart>,
    pub ends: Vec<ParsedEnd>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedStart {
    pub launch: LaunchId,
    pub spec: StartSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEnd {
    pub launch: LaunchId,
    pub spec: EndSpec,
}

#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    /// `mischief-journal.jsonl` is over [`MAX_JOURNAL_BYTES`]. Nothing was read.
    TooLarge(u64),
}

/// Length first. Over [`MAX_JOURNAL_BYTES`], the file is not read.
pub trait JournalSource {
    fn byte_len(&self) -> io::Result<u64>;
    fn read_bytes(&self) -> io::Result<Vec<u8>>;
}

pub fn load_from_source(source: &impl JournalSource) -> Result<ParsedJournal, LoadError> {
    let len = source.byte_len().map_err(LoadError::Io)?;
    if len > MAX_JOURNAL_BYTES {
        return Err(LoadError::TooLarge(len));
    }
    let bytes = source.read_bytes().map_err(LoadError::Io)?;
    Ok(parse_journal(&bytes))
}

/// A missing file is an empty journal. A file over 1 MB is [`LoadError::TooLarge`].
pub fn load_journal(path: &Path) -> Result<ParsedJournal, LoadError> {
    match fs::metadata(path) {
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(ParsedJournal::default()),
        Err(err) => return Err(LoadError::Io(err)),
        Ok(_) => {}
    }
    load_from_source(&PathSource(path))
}

struct PathSource<'a>(&'a Path);

impl JournalSource for PathSource<'_> {
    fn byte_len(&self) -> io::Result<u64> {
        Ok(fs::metadata(self.0)?.len())
    }

    fn read_bytes(&self) -> io::Result<Vec<u8>> {
        fs::read(self.0)
    }
}

/// Parse journal bytes. A last line with no newline is dropped. A line that
/// is not valid JSON is skipped. Reading stops after [`MAX_JOURNAL_LINES`].
pub fn parse_journal(bytes: &[u8]) -> ParsedJournal {
    let bytes = complete_lines(bytes);
    let mut parsed = ParsedJournal::default();
    let mut current: Option<LaunchId> = None;
    let mut seen = 0usize;
    for raw in bytes.split(|byte| *byte == b'\n') {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        if seen >= MAX_JOURNAL_LINES {
            break;
        }
        seen += 1;
        let Ok(line) = serde_json::from_slice::<Line>(raw) else {
            continue;
        };
        match line {
            Line::Header { v, launch } => {
                current = (v == JOURNAL_VERSION)
                    .then(|| LaunchId::parse(&launch))
                    .flatten();
                if current.is_some() {
                    parsed.last_launch = current;
                }
            }
            Line::Start {
                v,
                op,
                pid,
                window,
                slot,
                x,
                y,
                w,
                h,
                t,
            } => {
                if v != JOURNAL_VERSION {
                    continue;
                }
                let Some(launch) = current else {
                    continue;
                };
                parsed.starts.push(ParsedStart {
                    launch,
                    spec: StartSpec {
                        op,
                        pid,
                        window,
                        slot,
                        bounds: Bounds {
                            x,
                            y,
                            width: w,
                            height: h,
                        },
                        at_ms: t,
                    },
                });
            }
            Line::End {
                v,
                op,
                pid,
                window,
                slot,
                t,
            } => {
                if v != JOURNAL_VERSION {
                    continue;
                }
                let Some(launch) = current else {
                    continue;
                };
                parsed.ends.push(ParsedEnd {
                    launch,
                    spec: EndSpec {
                        op,
                        pid,
                        window,
                        slot,
                        at_ms: t,
                    },
                });
            }
        }
    }
    parsed
}

fn complete_lines(bytes: &[u8]) -> &[u8] {
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return bytes;
    }
    match bytes.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => &bytes[..=index],
        None => &[],
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Line {
    Header {
        v: u32,
        launch: String,
    },
    Start {
        v: u32,
        op: Operation,
        pid: i32,
        window: u32,
        slot: u16,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        t: u64,
    },
    End {
        v: u32,
        op: Operation,
        pid: i32,
        window: u32,
        slot: u16,
        t: u64,
    },
}

impl Line {
    fn from_start(spec: &StartSpec) -> Self {
        Self::Start {
            v: JOURNAL_VERSION,
            op: spec.op,
            pid: spec.pid,
            window: spec.window,
            slot: spec.slot,
            x: spec.bounds.x,
            y: spec.bounds.y,
            w: spec.bounds.width,
            h: spec.bounds.height,
            t: spec.at_ms,
        }
    }

    fn from_end(spec: &EndSpec) -> Self {
        Self::End {
            v: JOURNAL_VERSION,
            op: spec.op,
            pid: spec.pid,
            window: spec.window,
            slot: spec.slot,
            t: spec.at_ms,
        }
    }
}

/// What the shell can see at restore time. Titles are `None` on every AX
/// window when the grant is missing.
pub trait LiveWindows {
    fn pid_alive(&self, pid: i32) -> bool;
    fn app_name(&self, pid: i32) -> Option<String>;
    fn cg_bounds(&self, pid: i32, window: u32) -> Option<Bounds>;
    fn ax_windows(&self, pid: i32) -> Vec<AxWindow>;
    fn visible_screens(&self) -> Vec<Bounds>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    pub moves: Vec<RestoreMove>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreMove {
    pub pid: i32,
    pub window: u32,
    pub slot: u16,
    pub op: Operation,
    pub to: Bounds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub pid: i32,
    pub window: u32,
    pub slot: u16,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    OtherSession,
    TooOld,
    DeadPid,
    UnknownApp,
    WindowGone,
    NoAppliedBounds,
    SlotMismatch,
    UserMoved,
    Ambiguous,
    NoUniqueWindow,
    NotSettable,
    Blocked(DenyReason),
    BadBounds,
    NoScreen,
    BadSlot,
}

/// Pure replay. `slots[i]` is carry slot `i`. The previous session is the
/// last header in `journal`. An end entry does not cancel a restore.
///
/// Starts from that session are grouped by `(pid, window)`. The target is
/// the earliest start's bounds: where the window sat before any pet touched
/// it. The user-moved check uses the slot record for that window with the
/// highest `seq` (the later slot when `seq` ties). A record whose
/// `(pid, window)` matches no start in the session is ignored.
pub fn replay(
    journal: &ParsedJournal,
    slots: &[Option<Applied>],
    rules: &WindowRules,
    live: &impl LiveWindows,
    now_ms: u64,
) -> RestorePlan {
    let mut plan = RestorePlan {
        moves: Vec::new(),
        skipped: Vec::new(),
    };
    let mut order = Vec::new();
    let mut chosen: HashMap<(i32, u32), &ParsedStart> = HashMap::new();
    for start in &journal.starts {
        if journal.last_launch != Some(start.launch) {
            plan.skipped.push(skip(start, SkipReason::OtherSession));
            continue;
        }
        if usize::from(start.spec.slot) >= SLOT_COUNT {
            plan.skipped.push(skip(start, SkipReason::BadSlot));
            continue;
        }
        let key = (start.spec.pid, start.spec.window);
        match chosen.entry(key) {
            Entry::Occupied(mut entry) => {
                if start.spec.at_ms < entry.get().spec.at_ms {
                    entry.insert(start);
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(start);
                order.push(key);
            }
        }
    }
    for key in order {
        let Some(start) = chosen.get(&key).copied() else {
            continue;
        };
        match restore_one(start, slots, rules, live, now_ms) {
            Ok(movement) => plan.moves.push(movement),
            Err(reason) => plan.skipped.push(skip(start, reason)),
        }
    }
    plan
}

/// Last bounds a pet applied to this window. Records for any other window
/// are ignored. Equal `seq` keeps the later slot in the table.
fn applied_for(slots: &[Option<Applied>], pid: i32, window: u32) -> Option<&Applied> {
    let mut found: Option<&Applied> = None;
    for record in slots.iter().flatten() {
        if record.pid != pid || record.window != window {
            continue;
        }
        match found {
            Some(current) if record.seq < current.seq => {}
            _ => found = Some(record),
        }
    }
    found
}

fn slot_record(slots: &[Option<Applied>], index: u16) -> Option<&Applied> {
    slots.get(usize::from(index)).and_then(|slot| slot.as_ref())
}

fn skip(start: &ParsedStart, reason: SkipReason) -> Skipped {
    Skipped {
        pid: start.spec.pid,
        window: start.spec.window,
        slot: start.spec.slot,
        reason,
    }
}

fn restore_one(
    start: &ParsedStart,
    slots: &[Option<Applied>],
    rules: &WindowRules,
    live: &impl LiveWindows,
    now_ms: u64,
) -> Result<RestoreMove, SkipReason> {
    let spec = &start.spec;
    if spec.at_ms <= now_ms && now_ms.saturating_sub(spec.at_ms) >= MAX_ENTRY_AGE_MS {
        return Err(SkipReason::TooOld);
    }
    if spec.pid <= 0 || !live.pid_alive(spec.pid) {
        return Err(SkipReason::DeadPid);
    }
    let Some(app) = live.app_name(spec.pid) else {
        return Err(SkipReason::UnknownApp);
    };
    let Some(current) = live.cg_bounds(spec.pid, spec.window) else {
        return Err(SkipReason::WindowGone);
    };
    let Some(applied) = applied_for(slots, spec.pid, spec.window) else {
        if slot_record(slots, spec.slot)
            .is_some_and(|record| record.pid != spec.pid || record.window != spec.window)
        {
            return Err(SkipReason::SlotMismatch);
        }
        return Err(SkipReason::NoAppliedBounds);
    };
    if applied.bounds != current {
        return Err(SkipReason::UserMoved);
    }
    if spec.bounds.width <= 0 || spec.bounds.height <= 0 {
        return Err(SkipReason::BadBounds);
    }
    let screens = live.visible_screens();
    if screens.is_empty() {
        return Err(SkipReason::NoScreen);
    }
    let windows = live.ax_windows(spec.pid);
    if let Err(deny) = assess_carry(rules, &app, current, &windows) {
        return Err(match deny {
            CarryDeny::Ax(AxMiss::Ambiguous) => SkipReason::Ambiguous,
            CarryDeny::Ax(AxMiss::NoUniqueWindow) => SkipReason::NoUniqueWindow,
            CarryDeny::NotSettable => SkipReason::NotSettable,
            CarryDeny::Rules(reason) => SkipReason::Blocked(reason),
        });
    }
    let Some(to) = clamp_partly_on_screen(spec.bounds, &screens) else {
        return Err(SkipReason::NoScreen);
    };
    if !partly_on_screen(to, &screens) {
        return Err(SkipReason::BadBounds);
    }
    Ok(RestoreMove {
        pid: spec.pid,
        window: spec.window,
        slot: spec.slot,
        op: spec.op,
        to,
    })
}

/// Load the journal and the slots, then [`replay`]. A journal over 1 MB
/// returns before the slots or `live` are touched.
pub fn restore_directory(
    dir: &Path,
    rules: &WindowRules,
    live: &impl LiveWindows,
    now_ms: u64,
) -> Result<RestorePlan, LoadError> {
    let parsed = load_journal(&dir.join(JOURNAL_FILE))?;
    let slots = read_slot_file(&dir.join(SLOTS_FILE)).map_err(LoadError::Io)?;
    Ok(replay(&parsed, &slots, rules, live, now_ms))
}

pub fn read_slot_file(path: &Path) -> io::Result<Vec<Option<Applied>>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut file = FileSlots::open_read(path)?;
    let mut slots = Vec::with_capacity(SLOT_COUNT);
    for index in 0..SLOT_COUNT {
        slots.push(read_applied(&mut file, index as u16)?);
    }
    Ok(slots)
}

/// Empty both mischief files after the shell has applied a restore.
/// A refused journal is left for the caller to delete.
pub fn clear_journal_files(dir: &Path) -> io::Result<()> {
    truncate_private(&dir.join(JOURNAL_FILE))?;
    truncate_private(&dir.join(SLOTS_FILE))?;
    Ok(())
}

fn truncate_private(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(0)?;
    tighten(&file)
}

pub fn journal_path(dir: &Path) -> PathBuf {
    dir.join(JOURNAL_FILE)
}

pub fn slots_path(dir: &Path) -> PathBuf {
    dir.join(SLOTS_FILE)
}

fn create_private(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    tighten(&file)?;
    Ok(file)
}

fn tighten(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = file;
    }
    Ok(())
}

pub struct FileLog {
    file: File,
}

impl FileLog {
    pub fn create(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: create_private(path)?,
        })
    }
}

impl LogIo for FileLog {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        use std::io::{Seek, SeekFrom};
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(bytes)
    }

    fn sync_log(&mut self) -> io::Result<()> {
        self.file.sync_all()
    }
}

pub struct FileSlots {
    file: File,
}

impl FileSlots {
    /// Create or reopen the slot file for writing. An existing table is kept.
    /// The file is extended to [`SLOT_COUNT`] records when it is shorter.
    /// Nothing is fsynced.
    pub fn create(path: &Path) -> io::Result<Self> {
        let file = create_private(path)?;
        let need = SLOT_COUNT as u64 * SLOT_LEN as u64;
        if file.metadata()?.len() < need {
            file.set_len(need)?;
        }
        Ok(Self { file })
    }

    fn open_read(path: &Path) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
        })
    }
}

impl SlotIo for FileSlots {
    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        write_all_at(&self.file, offset, bytes)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        read_some_at(&self.file, offset, buf)
    }
}

#[derive(Debug, Default)]
pub struct MemoryLog {
    data: Vec<u8>,
    fail_writes: bool,
}

impl MemoryLog {
    pub fn fail_writes(&mut self) {
        self.fail_writes = true;
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
}

impl LogIo for MemoryLog {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.fail_writes {
            return Err(io::Error::new(ErrorKind::StorageFull, "disk full"));
        }
        self.data.extend_from_slice(bytes);
        Ok(())
    }

    fn sync_log(&mut self) -> io::Result<()> {
        if self.fail_writes {
            return Err(io::Error::new(ErrorKind::StorageFull, "disk full"));
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct MemorySlots {
    data: Vec<u8>,
}

impl SlotIo for MemorySlots {
    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        let start = usize::try_from(offset)
            .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "offset"))?;
        let end = start
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "offset"))?;
        if self.data.len() < end {
            self.data.resize(end, 0);
        }
        self.data[start..end].copy_from_slice(bytes);
        Ok(())
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let start = usize::try_from(offset)
            .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "offset"))?;
        if start >= self.data.len() {
            buf.fill(0);
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        if n < buf.len() {
            buf[n..].fill(0);
        }
        Ok(n)
    }
}

fn write_all_at(file: &File, mut offset: u64, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let n = write_some_at(file, offset, bytes)?;
        if n == 0 {
            return Err(io::Error::new(ErrorKind::WriteZero, "positioned write"));
        }
        offset += n as u64;
        bytes = &bytes[n..];
    }
    Ok(())
}

fn write_some_at(file: &File, offset: u64, bytes: &[u8]) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        loop {
            match file.write_at(bytes, offset) {
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                other => return other,
            }
        }
    }
    #[cfg(not(unix))]
    {
        use std::io::{Seek, SeekFrom};
        let mut file = file.try_clone()?;
        file.seek(SeekFrom::Start(offset))?;
        loop {
            match file.write(bytes) {
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                other => return other,
            }
        }
    }
}

fn read_some_at(file: &File, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        let mut read = 0;
        while read < buf.len() {
            match file.read_at(&mut buf[read..], offset + read as u64) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            }
        }
        Ok(read)
    }
    #[cfg(not(unix))]
    {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = file.try_clone()?;
        file.seek(SeekFrom::Start(offset))?;
        let mut read = 0;
        while read < buf.len() {
            match file.read(&mut buf[read..]) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            }
        }
        Ok(read)
    }
}
