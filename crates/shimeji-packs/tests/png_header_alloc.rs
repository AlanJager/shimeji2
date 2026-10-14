//! A 100000×100000 IHDR must be rejected before the decoder allocates the image.
//! This binary has its own allocator so the 512 MB budget test cannot raise the peak.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use shimeji_packs::{CollisionPolicy, ImportFailure, import_pack};

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

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

#[test]
fn huge_png_header_is_rejected_before_decoding() {
    let tmp = std::env::temp_dir().join(format!(
        "shimeji-u5b-ihdr-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let source = tmp.join("mascot");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("actions.xml"), ACTIONS).unwrap();
    fs::write(source.join("behaviors.xml"), BEHAVIORS).unwrap();
    fs::write(source.join("huge.png"), huge_png_header()).unwrap();
    let packs_root = tmp.join("packs-root");

    let baseline = CURRENT.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let started = Instant::now();
    let error = import_pack(&source, &packs_root, CollisionPolicy::Report)
        .expect_err("huge header should reject the pack");
    let elapsed = started.elapsed();
    let peak = PEAK.load(Ordering::Relaxed);
    let delta = peak.saturating_sub(baseline);

    let _ = fs::remove_dir_all(&tmp);

    match error.failure {
        ImportFailure::ImageDimensions { width, height, .. } => {
            assert_eq!((width, height), (100_000, 100_000));
        }
        other => panic!("expected ImageDimensions, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(1),
        "header check took {elapsed:?}"
    );
    // A decoded 100000² RGBA image is 40 GB. A few megabytes is the import's
    // own scratch space, not that image.
    assert!(
        delta < 8 * 1024 * 1024,
        "allocated {delta} bytes while rejecting the header"
    );
    assert!(!packs_root.join("staging").exists() || dir_empty(&packs_root.join("staging")));
}

fn dir_empty(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|read| read.count() == 0)
        .unwrap_or(true)
}

fn huge_png_header() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&100_000u32.to_be_bytes());
    bytes.extend_from_slice(&100_000u32.to_be_bytes());
    bytes
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
