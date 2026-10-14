//! A WAV whose `data` chunk header claims hundreds of megabytes must not
//! allocate that claim. This binary has its own allocator so other tests
//! cannot raise the peak.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use shimeji_packs::{CollisionPolicy, MAX_DECODED_BYTES, Severity, import_pack};

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static GATE: Mutex<()> = Mutex::new(());

const PEAK_LIMIT: usize = 16 * 1024 * 1024;

fn note_alloc(size: usize) {
    let current = CURRENT.fetch_add(size, Ordering::Relaxed) + size;
    let mut peak = PEAK.load(Ordering::Relaxed);
    while current > peak {
        match PEAK.compare_exchange_weak(peak, current, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => peak = observed,
        }
    }
}

fn note_free(size: usize) {
    CURRENT.fetch_sub(size, Ordering::Relaxed);
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            note_alloc(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            note_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        note_free(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size > layout.size() {
                note_alloc(new_size - layout.size());
            } else {
                note_free(layout.size() - new_size);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "shimeji-u5b-wav-{label}-{}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn fmt_pcm() -> [u8; 16] {
    let mut fmt = [0u8; 16];
    fmt[0..2].copy_from_slice(&1u16.to_le_bytes());
    fmt[2..4].copy_from_slice(&1u16.to_le_bytes());
    fmt[4..8].copy_from_slice(&8_000u32.to_le_bytes());
    fmt[8..12].copy_from_slice(&16_000u32.to_le_bytes());
    fmt[12..14].copy_from_slice(&2u16.to_le_bytes());
    fmt[14..16].copy_from_slice(&16u16.to_le_bytes());
    fmt
}

/// 44-byte header plus 4 payload bytes. The `data` size field is `declared`.
fn lying_wav(declared: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&fmt_pcm());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&declared.to_le_bytes());
    body.extend_from_slice(&[1, 2, 3, 4]);
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(u32::try_from(body.len()).unwrap()).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(&body);
    assert_eq!(wav.len(), 48);
    wav
}

fn write_folder(root: &Path, wav: &[u8]) -> PathBuf {
    let source = root.join("mascot");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("actions.xml"), ACTIONS).unwrap();
    fs::write(source.join("behaviors.xml"), BEHAVIORS).unwrap();
    fs::write(source.join("clip.wav"), wav).unwrap();
    source
}

fn import_peak(source: &Path, packs_root: &Path) -> (shimeji_packs::ImportOutcome, usize) {
    let baseline = CURRENT.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let outcome = import_pack(source, packs_root, CollisionPolicy::Report)
        .expect("lying wav should import with a warning");
    let peak = PEAK.load(Ordering::Relaxed);
    (outcome, peak.saturating_sub(baseline))
}

fn assert_warned(outcome: &shimeji_packs::ImportOutcome, delta: usize) {
    assert!(
        outcome.diagnostics.iter().any(|diag| {
            diag.severity == Severity::Warning
                && diag.file == "clip.wav"
                && diag.message.contains("declares")
        }),
        "{:#?}",
        outcome.diagnostics
    );
    assert!(
        outcome.sounds.is_empty(),
        "decoded {} pcm bytes",
        outcome
            .sounds
            .iter()
            .map(|sound| sound.pcm.len())
            .sum::<usize>()
    );
    assert!(
        delta < PEAK_LIMIT,
        "allocated {delta} bytes while importing a tiny wav"
    );
    let installed = fs::metadata(outcome.path.join("clip.wav")).unwrap();
    assert!(installed.len() < 1024, "installed {}", installed.len());
}

#[test]
fn wav_declaring_400_mib_imports_with_a_warning_under_16_mib() {
    let _guard = GATE.lock().unwrap_or_else(|poison| poison.into_inner());
    let tmp = Tmp::new("400");
    let wav = lying_wav(400 * 1024 * 1024);
    let source = write_folder(&tmp.path, &wav);
    let packs_root = tmp.path.join("packs-root");
    let (outcome, delta) = import_peak(&source, &packs_root);
    assert_warned(&outcome, delta);
}

#[test]
fn wav_declaring_over_the_budget_with_a_tiny_payload_warns_under_16_mib() {
    let _guard = GATE.lock().unwrap_or_else(|poison| poison.into_inner());
    let declared = u32::try_from(MAX_DECODED_BYTES + 1).unwrap();
    let tmp = Tmp::new("budget");
    let wav = lying_wav(declared);
    let source = write_folder(&tmp.path, &wav);
    let packs_root = tmp.path.join("packs-root");
    let (outcome, delta) = import_peak(&source, &packs_root);
    assert_warned(&outcome, delta);
}

const ACTIONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="Sit" Type="Stay"/>
  </ActionList>
</Mascot>
"#;

const BEHAVIORS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="Sit" Frequency="0"/>
    <Behavior Name="Fall" Action="Sit" Frequency="0"/>
    <Behavior Name="Dragged" Action="Sit" Frequency="0"/>
    <Behavior Name="Thrown" Action="Sit" Frequency="0"/>
  </BehaviorList>
</Mascot>
"#;
