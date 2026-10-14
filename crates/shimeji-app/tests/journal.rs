//! Journal, slots, the carry gate, AX matching, and restore.

use std::cell::Cell;
use std::fs::{self, File};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use shimeji_app::journal::{
    AX_STANDARD_WINDOW, Applied, AxMiss, AxWindow, Bounds, CarryDeny, CarryRefusal, ElapsedClock,
    EndSpec, FileLog, FileSlots, JOURNAL_FILE, JournalSource, LaunchId, LiveWindows, LoadError,
    LogIo, MAX_ENTRY_AGE_MS, MAX_JOURNAL_BYTES, MemoryLog, MemorySlots, Operation, ParsedJournal,
    ParsedStart, SLOT_COUNT, SLOTS_FILE, START_SYNC_LIMIT, SkipReason, SlotAllocator, StartSpec,
    begin_carry, clear_journal_files, load_from_source, match_ax_window, parse_journal,
    partly_on_screen, read_applied, read_slot_file, replay, restore_directory, write_applied,
    write_end, write_header,
};
use shimeji_app::window_rules::{DenyReason, WindowRules};

const NOW: u64 = 1_700_000_000_000;

struct Tmp(PathBuf);

impl Tmp {
    fn new(label: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "shimeji-u11a-journal-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Frozen;

impl ElapsedClock for Frozen {
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
}

#[derive(Clone)]
struct ManualClock(Rc<Cell<Duration>>);

impl ManualClock {
    fn new() -> Self {
        Self(Rc::new(Cell::new(Duration::ZERO)))
    }

    fn advance(&self, by: Duration) {
        self.0.set(self.0.get().saturating_add(by));
    }
}

impl ElapsedClock for ManualClock {
    fn elapsed(&self) -> Duration {
        self.0.get()
    }
}

struct SlowSync {
    clock: ManualClock,
    delay: Duration,
    timed_out: bool,
}

impl LogIo for SlowSync {
    fn append(&mut self, _bytes: &[u8]) -> io::Result<()> {
        Ok(())
    }

    fn sync_log(&mut self) -> io::Result<()> {
        if self.timed_out {
            return Err(io::Error::new(ErrorKind::TimedOut, "fsync"));
        }
        self.clock.advance(self.delay);
        Ok(())
    }
}

struct CountLog<L> {
    inner: L,
    syncs: u32,
}

impl<L: LogIo> LogIo for CountLog<L> {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inner.append(bytes)
    }

    fn sync_log(&mut self) -> io::Result<()> {
        self.syncs += 1;
        self.inner.sync_log()
    }
}

struct Proc {
    pid: i32,
    alive: bool,
    app: String,
    cg: Vec<(u32, Bounds)>,
    ax: Vec<AxWindow>,
}

struct World {
    procs: Vec<Proc>,
    screens: Vec<Bounds>,
}

impl LiveWindows for World {
    fn pid_alive(&self, pid: i32) -> bool {
        self.procs
            .iter()
            .find(|proc| proc.pid == pid)
            .is_some_and(|proc| proc.alive)
    }

    fn app_name(&self, pid: i32) -> Option<String> {
        self.procs
            .iter()
            .find(|proc| proc.pid == pid)
            .map(|proc| proc.app.clone())
    }

    fn cg_bounds(&self, pid: i32, window: u32) -> Option<Bounds> {
        self.procs
            .iter()
            .find(|proc| proc.pid == pid)?
            .cg
            .iter()
            .find(|(id, _)| *id == window)
            .map(|(_, bounds)| *bounds)
    }

    fn ax_windows(&self, pid: i32) -> Vec<AxWindow> {
        self.procs
            .iter()
            .find(|proc| proc.pid == pid)
            .map(|proc| proc.ax.clone())
            .unwrap_or_default()
    }

    fn visible_screens(&self) -> Vec<Bounds> {
        self.screens.clone()
    }
}

fn screen() -> Bounds {
    Bounds {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    }
}

fn ax(bounds: Bounds, title: Option<&str>) -> AxWindow {
    AxWindow {
        bounds,
        subrole: AX_STANDARD_WINDOW.to_string(),
        position_settable: true,
        title: title.map(str::to_string),
    }
}

fn world_one(pid: i32, window: u32, app: &str, bounds: Bounds, title: Option<&str>) -> World {
    World {
        procs: vec![Proc {
            pid,
            alive: true,
            app: app.to_string(),
            cg: vec![(window, bounds)],
            ax: vec![ax(bounds, title)],
        }],
        screens: vec![screen()],
    }
}

fn start_at(
    op: Operation,
    pid: i32,
    window: u32,
    slot: u16,
    bounds: Bounds,
    at_ms: u64,
) -> StartSpec {
    StartSpec {
        op,
        pid,
        window,
        slot,
        bounds,
        at_ms,
    }
}

fn launch(byte: u8) -> LaunchId {
    LaunchId::from_bytes([byte; 16])
}

fn parsed(launch_id: LaunchId, starts: Vec<StartSpec>) -> ParsedJournal {
    ParsedJournal {
        last_launch: Some(launch_id),
        starts: starts
            .into_iter()
            .map(|spec| ParsedStart {
                launch: launch_id,
                spec,
            })
            .collect(),
        ends: Vec::new(),
    }
}

fn applied(pid: i32, window: u32, bounds: Bounds) -> Applied {
    Applied {
        pid,
        window,
        bounds,
        seq: 1,
    }
}

fn slots_of(entries: Vec<(u16, Applied)>) -> Vec<Option<Applied>> {
    let mut slots = vec![None; entries.len()];
    for (index, entry) in entries {
        let index = usize::from(index);
        if slots.len() <= index {
            slots.resize(index + 1, None);
        }
        slots[index] = Some(entry);
    }
    slots
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn carry_start_five_seconds_of_moves_and_end_replay_to_the_original() {
    let ticks = 25 * 5;
    assert_eq!(ticks, 125);

    let dir = Tmp::new("happy");
    let mut log = CountLog {
        inner: FileLog::create(&dir.path().join(JOURNAL_FILE)).unwrap(),
        syncs: 0,
    };
    let mut slots = FileSlots::create(&dir.path().join(SLOTS_FILE)).unwrap();
    #[cfg(unix)]
    {
        assert_eq!(mode(&dir.path().join(JOURNAL_FILE)), 0o600);
        assert_eq!(mode(&dir.path().join(SLOTS_FILE)), 0o600);
    }

    let id = launch(7);
    write_header(&mut log, id).unwrap();
    let original = Bounds {
        x: 40,
        y: 80,
        width: 400,
        height: 300,
    };
    let spec = start_at(Operation::Carry, 4242, 7, 0, original, NOW);
    begin_carry(&mut log, &Frozen, &spec).unwrap();

    let mut last = original;
    for tick in 0..ticks {
        last.x = original.x + tick + 1;
        write_applied(&mut slots, 0, spec.pid, spec.window, last).unwrap();
        let stored = read_applied(&mut slots, 0).unwrap().unwrap();
        assert_eq!(stored.bounds, last);
        assert_eq!(stored.seq, tick as u64 + 1);
    }
    write_end(
        &mut log,
        &EndSpec {
            op: Operation::Carry,
            pid: spec.pid,
            window: spec.window,
            slot: 0,
            at_ms: NOW + 5_000,
        },
    )
    .unwrap();
    assert_eq!(log.syncs, 3);
    drop(log);
    drop(slots);

    let text = fs::read_to_string(dir.path().join(JOURNAL_FILE)).unwrap();
    assert_eq!(text.lines().count(), 3);
    let journal = parse_journal(text.as_bytes());
    assert_eq!(journal.last_launch, Some(id));
    assert_eq!(journal.starts.len(), 1);
    assert_eq!(journal.starts[0].spec.bounds, original);
    assert_eq!(journal.ends.len(), 1);
    assert!(!text.contains(&last.x.to_string()));

    let live = world_one(spec.pid, spec.window, "TextEdit", last, Some("Untitled"));
    let plan = restore_directory(dir.path(), &WindowRules::default(), &live, NOW).unwrap();
    assert!(plan.skipped.is_empty());
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].op, Operation::Carry);
    assert_eq!(plan.moves[0].to, original);

    clear_journal_files(dir.path()).unwrap();
    assert_eq!(
        fs::metadata(dir.path().join(JOURNAL_FILE)).unwrap().len(),
        0
    );
    assert_eq!(fs::metadata(dir.path().join(SLOTS_FILE)).unwrap().len(), 0);
    #[cfg(unix)]
    {
        assert_eq!(mode(&dir.path().join(JOURNAL_FILE)), 0o600);
        assert_eq!(mode(&dir.path().join(SLOTS_FILE)), 0o600);
    }
}

#[test]
fn crash_mid_throw_keeps_the_last_applied_slot_and_restores() {
    let dir = Tmp::new("crash");
    let mut log = FileLog::create(&dir.path().join(JOURNAL_FILE)).unwrap();
    let mut slots = FileSlots::create(&dir.path().join(SLOTS_FILE)).unwrap();
    let id = launch(4);
    write_header(&mut log, id).unwrap();
    let original = Bounds {
        x: 40,
        y: 50,
        width: 220,
        height: 140,
    };
    let spec = start_at(Operation::Throw, 80, 3, 0, original, NOW);
    begin_carry(&mut log, &Frozen, &spec).unwrap();

    let mut last = original;
    for step in 1..=10 {
        last.x = original.x + step * 17;
        write_applied(&mut slots, 0, spec.pid, spec.window, last).unwrap();
    }
    assert_eq!(last.x, 210);
    drop(log);
    drop(slots);

    let text = fs::read_to_string(dir.path().join(JOURNAL_FILE)).unwrap();
    let journal = parse_journal(text.as_bytes());
    assert_eq!(journal.starts.len(), 1);
    assert!(journal.ends.is_empty());
    assert!(!text.contains("210"));
    let stored = read_slot_file(&dir.path().join(SLOTS_FILE)).unwrap();
    assert_eq!(stored[0].unwrap().bounds, last);
    assert_eq!(stored[0].unwrap().seq, 10);

    let live = world_one(spec.pid, spec.window, "TextEdit", last, Some("Untitled"));
    let plan = restore_directory(dir.path(), &WindowRules::default(), &live, NOW).unwrap();
    assert!(plan.skipped.is_empty());
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].op, Operation::Throw);
    assert_eq!(plan.moves[0].to, original);
}

#[test]
fn disk_full_or_slow_fsync_refuses_the_carry() {
    let spec = start_at(
        Operation::Carry,
        1,
        1,
        0,
        Bounds {
            x: 1,
            y: 2,
            width: 30,
            height: 40,
        },
        NOW,
    );

    let mut full = MemoryLog::default();
    full.fail_writes();
    let mut slots = MemorySlots::default();
    let mut moved = false;
    match begin_carry(&mut full, &Frozen, &spec) {
        Err(reason) => {
            assert_eq!(reason, CarryRefusal::WriteFailed);
            assert!(reason.window_stays_put());
            assert!(reason.pet_falls());
        }
        Ok(()) => moved = true,
    }
    assert!(!moved);
    assert!(full.bytes().is_empty());
    assert!(read_applied(&mut slots, 0).unwrap().is_none());

    let clock = ManualClock::new();
    let mut late = SlowSync {
        clock: clock.clone(),
        delay: Duration::from_millis(51),
        timed_out: false,
    };
    let mut moved = false;
    match begin_carry(&mut late, &clock, &spec) {
        Err(reason) => {
            assert_eq!(reason, CarryRefusal::SyncTimeout);
            assert!(reason.window_stays_put());
            assert!(reason.pet_falls());
        }
        Ok(()) => moved = true,
    }
    assert!(!moved);
    assert!(read_applied(&mut slots, 0).unwrap().is_none());

    let clock = ManualClock::new();
    let mut reported = SlowSync {
        clock: clock.clone(),
        delay: Duration::ZERO,
        timed_out: true,
    };
    assert_eq!(
        begin_carry(&mut reported, &clock, &spec).unwrap_err(),
        CarryRefusal::SyncTimeout
    );

    let clock = ManualClock::new();
    let mut on_time = SlowSync {
        clock: clock.clone(),
        delay: START_SYNC_LIMIT,
        timed_out: false,
    };
    begin_carry(&mut on_time, &clock, &spec).unwrap();
}

#[test]
fn two_standard_windows_at_the_same_bounds_are_not_a_unique_match() {
    let bounds = Bounds {
        x: 10,
        y: 20,
        width: 100,
        height: 80,
    };
    let near = AxWindow {
        bounds: Bounds { x: 11, ..bounds },
        subrole: AX_STANDARD_WINDOW.to_string(),
        position_settable: true,
        title: Some("Near".to_string()),
    };
    assert_eq!(match_ax_window(bounds, &[near]).unwrap(), 0);

    let far = AxWindow {
        bounds: Bounds { x: 12, ..bounds },
        ..ax(bounds, None)
    };
    assert_eq!(match_ax_window(bounds, &[far]), Err(AxMiss::NoUniqueWindow));

    let dialog = AxWindow {
        subrole: "AXDialog".to_string(),
        ..ax(bounds, Some("Sheet"))
    };
    let standard = ax(bounds, Some("Document"));
    assert_eq!(
        match_ax_window(bounds, &[dialog, standard.clone()]).unwrap(),
        1
    );

    let left = ax(bounds, Some("Left"));
    let right = ax(bounds, Some("Right"));
    assert_eq!(
        match_ax_window(bounds, &[left.clone(), right.clone()]),
        Err(AxMiss::Ambiguous)
    );
    let mut moved = false;
    match shimeji_app::journal::assess_carry(
        &WindowRules::default(),
        "TextEdit",
        bounds,
        &[left, right],
    ) {
        Err(CarryDeny::Ax(AxMiss::Ambiguous)) => {}
        Ok(_) => moved = true,
        Err(other) => panic!("unexpected {other:?}"),
    }
    assert!(!moved);

    let stuck = AxWindow {
        position_settable: false,
        ..ax(bounds, None)
    };
    assert_eq!(
        shimeji_app::journal::assess_carry(&WindowRules::default(), "TextEdit", bounds, &[stuck]),
        Err(CarryDeny::NotSettable)
    );
}

#[test]
fn a_window_the_user_moved_is_not_restored() {
    let original = Bounds {
        x: 20,
        y: 30,
        width: 200,
        height: 100,
    };
    let left_by_pet = Bounds {
        x: 80,
        y: 30,
        width: 200,
        height: 100,
    };
    let moved_by_user = Bounds {
        x: 400,
        y: 30,
        width: 200,
        height: 100,
    };
    let spec = start_at(Operation::Carry, 5, 8, 0, original, NOW);
    let journal = parsed(launch(1), vec![spec]);
    let slots = slots_of(vec![(0, applied(5, 8, left_by_pet))]);
    let live = world_one(5, 8, "TextEdit", moved_by_user, Some("Notes"));
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW);
    assert!(plan.moves.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason, SkipReason::UserMoved);
}

#[test]
fn an_entry_for_a_dead_pid_is_dropped() {
    let bounds = Bounds {
        x: 20,
        y: 30,
        width: 200,
        height: 100,
    };
    let spec = start_at(Operation::Carry, 5, 8, 0, bounds, NOW);
    let journal = parsed(launch(1), vec![spec]);
    let slots = slots_of(vec![(0, applied(5, 8, bounds))]);
    let mut live = world_one(5, 8, "TextEdit", bounds, Some("Notes"));
    live.procs[0].alive = false;
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW);
    assert!(plan.moves.is_empty());
    assert_eq!(plan.skipped[0].reason, SkipReason::DeadPid);
}

#[test]
fn two_windows_with_the_same_title_and_bounds_are_skipped() {
    let bounds = Bounds {
        x: 15,
        y: 25,
        width: 180,
        height: 90,
    };
    let spec = start_at(Operation::Throw, 6, 4, 0, bounds, NOW);
    let journal = parsed(launch(1), vec![spec]);
    let slots = slots_of(vec![(0, applied(6, 4, bounds))]);
    let mut live = world_one(6, 4, "TextEdit", bounds, Some("Notes"));
    live.procs[0].ax.push(ax(bounds, Some("Notes")));
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW);
    assert!(plan.moves.is_empty());
    assert_eq!(plan.skipped[0].reason, SkipReason::Ambiguous);
}

#[test]
fn a_truncated_last_line_is_ignored_and_earlier_entries_replay() {
    let dir = Tmp::new("truncated");
    let mut log = MemoryLog::default();
    let id = launch(9);
    write_header(&mut log, id).unwrap();
    let bounds = Bounds {
        x: 12,
        y: 16,
        width: 240,
        height: 120,
    };
    let spec = start_at(Operation::Carry, 15, 2, 0, bounds, NOW);
    begin_carry(&mut log, &Frozen, &spec).unwrap();
    let mut bytes = log.bytes().to_vec();
    bytes.extend_from_slice(
        br#"{"kind":"start","v":1,"op":"throw","pid":99,"window":8,"slot":1,"x":1,"y":2,"w":10,"h":10,"t":1}"#,
    );
    assert!(!bytes.ends_with(b"\n"));
    fs::write(dir.path().join(JOURNAL_FILE), &bytes).unwrap();

    let mut slots = FileSlots::create(&dir.path().join(SLOTS_FILE)).unwrap();
    write_applied(&mut slots, 0, spec.pid, spec.window, bounds).unwrap();
    drop(slots);

    let journal = parse_journal(&bytes);
    assert_eq!(journal.starts.len(), 1);
    assert_eq!(journal.starts[0].spec.pid, 15);

    let live = world_one(spec.pid, spec.window, "TextEdit", bounds, None);
    let plan = restore_directory(dir.path(), &WindowRules::default(), &live, NOW).unwrap();
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].window, 2);
    assert!(plan.skipped.is_empty());
}

#[test]
fn hand_written_system_settings_and_1password_entries_are_skipped() {
    let dir = Tmp::new("blocked");
    let id = launch(2);
    let bounds = Bounds {
        x: 30,
        y: 40,
        width: 300,
        height: 180,
    };
    let body = format!(
        concat!(
            "{{\"kind\":\"header\",\"v\":1,\"launch\":\"{launch}\"}}\n",
            "{{\"kind\":\"start\",\"v\":1,\"op\":\"carry\",\"pid\":10,\"window\":1,\"slot\":0,",
            "\"x\":30,\"y\":40,\"w\":300,\"h\":180,\"t\":{t},\"app\":\"TextEdit\",\"title\":\"Privacy\"}}\n",
            "{{\"kind\":\"start\",\"v\":1,\"op\":\"throw\",\"pid\":11,\"window\":2,\"slot\":1,",
            "\"x\":30,\"y\":40,\"w\":300,\"h\":180,\"t\":{t},\"app\":\"TextEdit\",\"title\":\"Vault\"}}\n",
        ),
        launch = id.to_hex(),
        t = NOW,
    );
    fs::write(dir.path().join(JOURNAL_FILE), &body).unwrap();
    let mut slots = FileSlots::create(&dir.path().join(SLOTS_FILE)).unwrap();
    write_applied(&mut slots, 0, 10, 1, bounds).unwrap();
    write_applied(&mut slots, 1, 11, 2, bounds).unwrap();
    drop(slots);

    let live = World {
        procs: vec![
            Proc {
                pid: 10,
                alive: true,
                app: "System Settings".to_string(),
                cg: vec![(1, bounds)],
                ax: vec![ax(bounds, Some("Privacy & Security"))],
            },
            Proc {
                pid: 11,
                alive: true,
                app: "1Password".to_string(),
                cg: vec![(2, bounds)],
                ax: vec![ax(bounds, Some("Vault"))],
            },
        ],
        screens: vec![screen()],
    };
    let plan = restore_directory(dir.path(), &WindowRules::default(), &live, NOW).unwrap();
    assert!(plan.moves.is_empty());
    assert_eq!(plan.skipped.len(), 2);
    assert_eq!(
        plan.skipped[0].reason,
        SkipReason::Blocked(DenyReason::Immutable)
    );
    assert_eq!(
        plan.skipped[1].reason,
        SkipReason::Blocked(DenyReason::DefaultApp)
    );
}

#[test]
fn an_off_screen_restore_target_is_clamped() {
    let original = Bounds {
        x: 20_000,
        y: 20_000,
        width: 200,
        height: 100,
    };
    let parked = Bounds {
        x: 100,
        y: 100,
        width: 200,
        height: 100,
    };
    let spec = start_at(Operation::Throw, 4, 6, 0, original, NOW);
    let journal = parsed(launch(1), vec![spec]);
    let slots = slots_of(vec![(0, applied(4, 6, parked))]);
    let live = world_one(4, 6, "TextEdit", parked, Some("Untitled"));
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW);
    assert!(plan.skipped.is_empty());
    let to = plan.moves[0].to;
    assert_ne!(to, original);
    assert!(partly_on_screen(to, &[screen()]));
}

#[test]
fn a_50mb_journal_is_refused_and_nothing_moves() {
    struct Boom {
        reads: Rc<Cell<u32>>,
    }
    impl JournalSource for Boom {
        fn byte_len(&self) -> io::Result<u64> {
            Ok(50 * 1024 * 1024)
        }
        fn read_bytes(&self) -> io::Result<Vec<u8>> {
            self.reads.set(self.reads.get() + 1);
            panic!("refused journal was read");
        }
    }
    let reads = Rc::new(Cell::new(0));
    let err = load_from_source(&Boom {
        reads: Rc::clone(&reads),
    })
    .unwrap_err();
    assert!(matches!(
        err,
        LoadError::TooLarge(len) if len == 50 * 1024 * 1024 && len > MAX_JOURNAL_BYTES
    ));
    assert_eq!(reads.get(), 0);

    let dir = Tmp::new("huge");
    let path = dir.path().join(JOURNAL_FILE);
    let file = File::create(&path).unwrap();
    file.set_len(50 * 1024 * 1024).unwrap();
    drop(file);

    struct Untouched;
    impl LiveWindows for Untouched {
        fn pid_alive(&self, _: i32) -> bool {
            panic!("live windows were consulted");
        }
        fn app_name(&self, _: i32) -> Option<String> {
            panic!("live windows were consulted");
        }
        fn cg_bounds(&self, _: i32, _: u32) -> Option<Bounds> {
            panic!("live windows were consulted");
        }
        fn ax_windows(&self, _: i32) -> Vec<AxWindow> {
            panic!("live windows were consulted");
        }
        fn visible_screens(&self) -> Vec<Bounds> {
            panic!("live windows were consulted");
        }
    }

    let err = restore_directory(dir.path(), &WindowRules::default(), &Untouched, NOW).unwrap_err();
    assert!(matches!(err, LoadError::TooLarge(len) if len == 50 * 1024 * 1024));
    assert!(path.exists());
    fs::remove_file(&path).unwrap();
    assert!(!path.exists());
}

#[test]
fn entries_from_before_the_previous_session_are_ignored() {
    let bounds = Bounds {
        x: 8,
        y: 9,
        width: 160,
        height: 90,
    };
    let old_id = launch(1);
    let new_id = launch(2);
    let old = start_at(Operation::Carry, 21, 1, 0, bounds, NOW);
    let current = start_at(Operation::Carry, 22, 2, 1, bounds, NOW);
    let journal = ParsedJournal {
        last_launch: Some(new_id),
        starts: vec![
            ParsedStart {
                launch: old_id,
                spec: old,
            },
            ParsedStart {
                launch: new_id,
                spec: current,
            },
        ],
        ends: Vec::new(),
    };
    let slots = slots_of(vec![
        (0, applied(21, 1, bounds)),
        (1, applied(22, 2, bounds)),
    ]);
    let live = World {
        procs: vec![
            Proc {
                pid: 21,
                alive: true,
                app: "TextEdit".to_string(),
                cg: vec![(1, bounds)],
                ax: vec![ax(bounds, None)],
            },
            Proc {
                pid: 22,
                alive: true,
                app: "TextEdit".to_string(),
                cg: vec![(2, bounds)],
                ax: vec![ax(bounds, None)],
            },
        ],
        screens: vec![screen()],
    };
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW);
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].pid, 22);
    assert!(
        plan.skipped
            .iter()
            .any(|skip| { skip.pid == 21 && skip.reason == SkipReason::OtherSession })
    );
}

#[test]
fn an_entry_older_than_24_hours_is_ignored() {
    let bounds = Bounds {
        x: 8,
        y: 9,
        width: 160,
        height: 90,
    };
    let stale = start_at(Operation::Carry, 30, 1, 0, bounds, NOW - MAX_ENTRY_AGE_MS);
    let fresh = start_at(
        Operation::Carry,
        31,
        2,
        1,
        bounds,
        NOW - MAX_ENTRY_AGE_MS + 1,
    );
    let journal = parsed(launch(3), vec![stale, fresh]);
    let slots = slots_of(vec![
        (0, applied(30, 1, bounds)),
        (1, applied(31, 2, bounds)),
    ]);
    let live = World {
        procs: vec![
            Proc {
                pid: 30,
                alive: true,
                app: "TextEdit".to_string(),
                cg: vec![(1, bounds)],
                ax: vec![ax(bounds, None)],
            },
            Proc {
                pid: 31,
                alive: true,
                app: "TextEdit".to_string(),
                cg: vec![(2, bounds)],
                ax: vec![ax(bounds, None)],
            },
        ],
        screens: vec![screen()],
    };
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW);
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].pid, 31);
    assert_eq!(plan.skipped[0].reason, SkipReason::TooOld);
}

#[test]
fn no_journal_line_contains_a_window_title() {
    let secret = "Quarterly Results — Confidential";
    let dir = Tmp::new("title");
    let mut log = FileLog::create(&dir.path().join(JOURNAL_FILE)).unwrap();
    write_header(&mut log, launch(8)).unwrap();
    let bounds = Bounds {
        x: 4,
        y: 5,
        width: 100,
        height: 80,
    };
    let spec = start_at(Operation::Carry, 50, 3, 0, bounds, NOW);
    begin_carry(&mut log, &Frozen, &spec).unwrap();
    write_end(
        &mut log,
        &EndSpec {
            op: Operation::Carry,
            pid: spec.pid,
            window: spec.window,
            slot: 0,
            at_ms: NOW + 1,
        },
    )
    .unwrap();
    drop(log);

    let mut slots = FileSlots::create(&dir.path().join(SLOTS_FILE)).unwrap();
    write_applied(&mut slots, 0, spec.pid, spec.window, bounds).unwrap();
    drop(slots);

    let text = fs::read_to_string(dir.path().join(JOURNAL_FILE)).unwrap();
    assert!(text.contains("\"kind\":\"header\"") || text.contains("\"kind\": \"header\""));
    for line in text.lines() {
        assert!(!line.contains(secret), "{line}");
        assert!(!line.contains("\"title\""), "{line}");
        assert!(!line.contains("Confidential"), "{line}");
    }
    let live = world_one(spec.pid, spec.window, "TextEdit", bounds, Some(secret));
    let plan = restore_directory(dir.path(), &WindowRules::default(), &live, NOW).unwrap();
    assert_eq!(plan.moves.len(), 1);
    let again = fs::read_to_string(dir.path().join(JOURNAL_FILE)).unwrap();
    assert!(!again.contains(secret));
}

#[test]
fn memory_slots_round_trip_without_a_file() {
    let mut slots = MemorySlots::default();
    let bounds = Bounds {
        x: 3,
        y: 4,
        width: 5,
        height: 6,
    };
    write_applied(&mut slots, 2, 9, 8, bounds).unwrap();
    let stored = read_applied(&mut slots, 2).unwrap().unwrap();
    assert_eq!(stored.bounds, bounds);
    assert_eq!(stored.seq, 1);
    assert!(read_applied(&mut slots, 0).unwrap().is_none());
}

#[test]
fn two_carries_of_one_window_restore_the_first_original() {
    let original = Bounds {
        x: 20,
        y: 30,
        width: 200,
        height: 100,
    };
    let after_carry = Bounds {
        x: 300,
        y: 30,
        width: 200,
        height: 100,
    };
    let after_throw = Bounds {
        x: 900,
        y: 400,
        width: 200,
        height: 100,
    };
    let journal = parsed(
        launch(1),
        vec![
            start_at(Operation::Carry, 5, 8, 0, original, NOW),
            start_at(Operation::Throw, 5, 8, 1, after_carry, NOW + 1),
        ],
    );
    let slots = slots_of(vec![
        (0, applied(5, 8, after_carry)),
        (1, applied(5, 8, after_throw)),
    ]);
    let live = world_one(5, 8, "TextEdit", after_throw, Some("Notes"));
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW + 1);
    assert!(plan.skipped.is_empty());
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].pid, 5);
    assert_eq!(plan.moves[0].window, 8);
    assert_eq!(plan.moves[0].to, original);
}

#[test]
fn a_reused_slot_for_one_window_restores_the_first_original() {
    let original = Bounds {
        x: 20,
        y: 30,
        width: 200,
        height: 100,
    };
    let after_carry = Bounds {
        x: 300,
        y: 30,
        width: 200,
        height: 100,
    };
    let after_throw = Bounds {
        x: 900,
        y: 400,
        width: 200,
        height: 100,
    };
    let journal = parsed(
        launch(1),
        vec![
            start_at(Operation::Carry, 5, 8, 0, original, NOW),
            start_at(Operation::Throw, 5, 8, 0, after_carry, NOW + 1),
        ],
    );
    let slots = slots_of(vec![(0, applied(5, 8, after_throw))]);
    let live = world_one(5, 8, "TextEdit", after_throw, Some("Notes"));
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW + 1);
    assert!(plan.skipped.is_empty());
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].to, original);
}

#[test]
fn the_allocator_never_gives_one_slot_to_two_windows() {
    let mut alloc = SlotAllocator::new();
    let first = alloc.assign(5, 8).unwrap();
    assert_eq!(alloc.assign(5, 8).unwrap(), first);

    let other_window = alloc.assign(5, 9).unwrap();
    assert_ne!(other_window, first);
    let other_pid = alloc.assign(6, 8).unwrap();
    assert_ne!(other_pid, first);
    assert_ne!(other_pid, other_window);

    for n in 3..SLOT_COUNT {
        let slot = alloc.assign(100 + n as i32, n as u32).unwrap();
        assert_eq!(usize::from(slot), n);
    }
    assert_eq!(alloc.assign(5, 8).unwrap(), first);

    let refused = alloc.assign(7, 1).unwrap_err();
    assert_eq!(refused, CarryRefusal::NoFreeSlot);
    assert!(refused.window_stays_put());
    assert!(refused.pet_falls());
    assert_eq!(alloc.assign(5, 8).unwrap(), first);
}

#[test]
fn two_windows_each_carried_twice_both_restore() {
    let a_original = Bounds {
        x: 10,
        y: 20,
        width: 200,
        height: 100,
    };
    let a_mid = Bounds {
        x: 110,
        y: 20,
        width: 200,
        height: 100,
    };
    let a_final = Bounds {
        x: 210,
        y: 120,
        width: 200,
        height: 100,
    };
    let b_original = Bounds {
        x: 30,
        y: 40,
        width: 180,
        height: 90,
    };
    let b_mid = Bounds {
        x: 130,
        y: 40,
        width: 180,
        height: 90,
    };
    let b_final = Bounds {
        x: 230,
        y: 140,
        width: 180,
        height: 90,
    };
    let journal = parsed(
        launch(4),
        vec![
            start_at(Operation::Carry, 5, 1, 0, a_original, NOW),
            start_at(Operation::Carry, 6, 2, 1, b_original, NOW + 1),
            start_at(Operation::Throw, 5, 1, 2, a_mid, NOW + 2),
            start_at(Operation::Throw, 6, 2, 3, b_mid, NOW + 3),
        ],
    );
    let mut a_last = applied(5, 1, a_final);
    a_last.seq = 9;
    let mut b_last = applied(6, 2, b_final);
    b_last.seq = 8;
    let slots = slots_of(vec![
        (0, a_last),
        (1, b_last),
        (2, applied(5, 1, a_mid)),
        (3, applied(6, 2, b_mid)),
    ]);
    let live = World {
        procs: vec![
            Proc {
                pid: 5,
                alive: true,
                app: "TextEdit".to_string(),
                cg: vec![(1, a_final)],
                ax: vec![ax(a_final, None)],
            },
            Proc {
                pid: 6,
                alive: true,
                app: "TextEdit".to_string(),
                cg: vec![(2, b_final)],
                ax: vec![ax(b_final, None)],
            },
        ],
        screens: vec![screen()],
    };
    let plan = replay(&journal, &slots, &WindowRules::default(), &live, NOW + 3);
    assert!(plan.skipped.is_empty());
    assert_eq!(plan.moves.len(), 2);
    assert_eq!(plan.moves[0].pid, 5);
    assert_eq!(plan.moves[0].to, a_original);
    assert_eq!(plan.moves[1].pid, 6);
    assert_eq!(plan.moves[1].to, b_original);
}
