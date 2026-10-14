//! Hostile inputs are built in the test. Nothing under tests/fixtures is a zip
//! bomb, a symlink, or a FIFO.

#[path = "common/mod.rs"]
mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use shimeji_packs::{
    CollisionPolicy, DiagnosticKind, ImportError, ImportFailure, MAX_DECODED_BYTES, MAX_DEPTH,
    MAX_UNCOMPRESSED_BYTES, import_pack, load_dir,
};

static TMP_SEQ: AtomicU64 = AtomicU64::new(1);

const SIT: &str = r#"    <Action Name="Sit" Type="Stay"/>"#;

struct Tmp {
    path: PathBuf,
}

impl Tmp {
    fn new(label: &str) -> Self {
        let n = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("shimeji-u5b-{label}-{n}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn actions_xml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
{body}
  </ActionList>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="Sit" Frequency="0"/>
    <Behavior Name="Fall" Action="Sit" Frequency="0"/>
    <Behavior Name="Dragged" Action="Sit" Frequency="0"/>
    <Behavior Name="Thrown" Action="Sit" Frequency="0"/>
  </BehaviorList>
</Mascot>
"#
    )
}

fn behaviors_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <BehaviorList>
    <Behavior Name="ChaseMouse" Action="Sit" Frequency="0"/>
    <Behavior Name="Fall" Action="Sit" Frequency="0"/>
    <Behavior Name="Dragged" Action="Sit" Frequency="0"/>
    <Behavior Name="Thrown" Action="Sit" Frequency="0"/>
  </BehaviorList>
</Mascot>
"#
    .to_string()
}

fn write_pack(dir: &Path, actions_body: &str) {
    common::write_minimal_en(dir, actions_body);
}

struct Entry {
    name: Vec<u8>,
    method: u16,
    flags: u16,
    uncompressed_size: u32,
    crc: u32,
    payload: Vec<u8>,
    made_by: u16,
    external: u32,
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn stored_utf8(name: &str, data: &[u8]) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        method: 0,
        flags: 1 << 11,
        uncompressed_size: data.len() as u32,
        crc: crc32(data),
        payload: data.to_vec(),
        made_by: (3 << 8) | 20,
        external: 0,
    }
}

fn zip_bytes(entries: &[Entry]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = local.len() as u32;
        local.extend_from_slice(b"PK\x03\x04");
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&entry.flags.to_le_bytes());
        local.extend_from_slice(&entry.method.to_le_bytes());
        local.extend_from_slice(&0u32.to_le_bytes());
        local.extend_from_slice(&entry.crc.to_le_bytes());
        local.extend_from_slice(&(entry.payload.len() as u32).to_le_bytes());
        local.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
        local.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&entry.name);
        local.extend_from_slice(&entry.payload);

        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&entry.made_by.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&entry.flags.to_le_bytes());
        central.extend_from_slice(&entry.method.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&entry.crc.to_le_bytes());
        central.extend_from_slice(&(entry.payload.len() as u32).to_le_bytes());
        central.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
        central.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&entry.external.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&entry.name);
    }
    let cd_start = u32::try_from(local.len()).unwrap();
    let cd_size = u32::try_from(central.len()).unwrap();
    let count = u16::try_from(entries.len()).unwrap();
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn assert_uninstalled(packs_root: &Path) {
    assert!(
        !packs_root.join("staging").exists(),
        "staging directory was left behind"
    );
    let packs = packs_root.join("packs");
    if packs.exists() {
        let left: Vec<_> = fs::read_dir(&packs)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(left.is_empty(), "packs/ was not empty: {left:?}");
    }
}

fn tree_has_bytes(root: &Path, needle: &[u8]) -> bool {
    if !root.exists() || needle.is_empty() {
        return false;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            } else if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                if let Ok(bytes) = fs::read(&path) {
                    if bytes.windows(needle.len()).any(|window| window == needle) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn reject(source: &Path, packs_root: &Path) -> ImportError {
    import_pack(source, packs_root, CollisionPolicy::Report).expect_err("import should fail")
}

#[test]
fn zip_slip_is_rejected_and_writes_nothing_outside() {
    let tmp = Tmp::new("slip");
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("marker"), b"clean").unwrap();

    let zip_path = tmp.path().join("slip.zip");
    fs::write(
        &zip_path,
        zip_bytes(&[stored_utf8("../../evil", b"pwned-zip-slip")]),
    )
    .unwrap();

    let error = reject(&zip_path, &packs_root);
    assert!(
        matches!(error.failure, ImportFailure::UnsafePath { .. }),
        "{error:?}"
    );
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diag| diag.message.contains("../../evil")),
        "{:?}",
        error.diagnostics
    );
    assert_uninstalled(&packs_root);
    // The archive itself contains the payload. What must not exist is an
    // extracted file of that name anywhere outside the zip.
    assert!(
        files_named(tmp.path(), "evil").is_empty(),
        "zip-slip wrote {:?}",
        files_named(tmp.path(), "evil")
    );
    assert_eq!(fs::read(outside.join("marker")).unwrap(), b"clean");
    assert!(!tree_has_bytes(&packs_root, b"pwned-zip-slip"));
    assert!(!tree_has_bytes(&outside, b"pwned-zip-slip"));
}

fn files_named(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if !root.exists() {
        return found;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if entry.file_name() == name {
                found.push(path.clone());
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    found
}

#[test]
fn declared_size_is_rejected_before_extraction() {
    let tmp = Tmp::new("declared");
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();
    let declared = u32::try_from(MAX_UNCOMPRESSED_BYTES + 1).unwrap();
    let zip_path = tmp.path().join("bomb.zip");
    fs::write(
        &zip_path,
        zip_bytes(&[Entry {
            name: b"bomb.bin".to_vec(),
            method: 8,
            flags: 0,
            uncompressed_size: declared,
            crc: 0,
            payload: b"not-deflate".to_vec(),
            made_by: 20,
            external: 0,
        }]),
    )
    .unwrap();

    let error = reject(&zip_path, &packs_root);
    match error.failure {
        ImportFailure::DeclaredSize { bytes } => {
            assert!(bytes > MAX_UNCOMPRESSED_BYTES, "{bytes}");
        }
        other => panic!("expected DeclaredSize before extraction, got {other:?}"),
    }
    assert!(!error.diagnostics.is_empty());
    assert_uninstalled(&packs_root);
}

#[test]
fn lying_uncompressed_size_stops_during_extraction() {
    let compressed = deflate_zeros(MAX_UNCOMPRESSED_BYTES + 1);
    let tmp = Tmp::new("lying");
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();
    let zip_path = tmp.path().join("lie.zip");
    fs::write(
        &zip_path,
        zip_bytes(&[Entry {
            name: b"lie.bin".to_vec(),
            method: 8,
            flags: 0,
            uncompressed_size: 1,
            crc: 0,
            payload: compressed,
            made_by: 20,
            external: 0,
        }]),
    )
    .unwrap();

    let error = reject(&zip_path, &packs_root);
    match error.failure {
        ImportFailure::ActualSize { bytes } => {
            assert!(bytes > MAX_UNCOMPRESSED_BYTES, "{bytes}");
            assert!(
                bytes <= MAX_UNCOMPRESSED_BYTES + 64 * 1024,
                "stopped late: {bytes}"
            );
        }
        other => panic!("expected ActualSize during extraction, got {other:?}"),
    }
    assert_uninstalled(&packs_root);
}

fn deflate_zeros(len: u64) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::DeflateEncoder;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    let chunk = [0u8; 64 * 1024];
    let mut left = len;
    while left > 0 {
        let take = std::cmp::min(left, chunk.len() as u64) as usize;
        encoder.write_all(&chunk[..take]).unwrap();
        left -= take as u64;
    }
    encoder.finish().unwrap()
}

#[test]
fn zip_symlink_entry_is_rejected() {
    let tmp = Tmp::new("zlink");
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();
    let zip_path = tmp.path().join("link.zip");
    let mut entries = Vec::new();
    entries.push(Entry {
        name: b"id_rsa".to_vec(),
        method: 0,
        flags: 1 << 11,
        uncompressed_size: 4,
        crc: crc32(b"link"),
        payload: b"link".to_vec(),
        made_by: (3 << 8) | 20,
        external: 0o0120000 << 16,
    });
    entries.extend(minimal_entries());
    fs::write(&zip_path, zip_bytes(&entries)).unwrap();

    let error = reject(&zip_path, &packs_root);
    assert!(
        matches!(error.failure, ImportFailure::Symlink { .. }),
        "{error:?}"
    );
    assert!(
        error
            .diagnostics
            .iter()
            .any(|diag| diag.message.contains("id_rsa"))
    );
    assert_uninstalled(&packs_root);
}

fn minimal_entries() -> Vec<Entry> {
    let actions = actions_xml(SIT);
    let behaviors = behaviors_xml();
    vec![
        stored_utf8("actions.xml", actions.as_bytes()),
        stored_utf8("behaviors.xml", behaviors.as_bytes()),
        stored_utf8("img/p.png", common::PNG),
    ]
}

#[test]
#[cfg(unix)]
fn folder_symlink_to_outside_file_is_rejected() {
    let tmp = Tmp::new("flink");
    let secret = tmp.path().join("secret");
    fs::write(&secret, b"SECRET-KEY-MATERIAL").unwrap();
    let source = tmp.path().join("mascot");
    write_pack(&source, SIT);
    std::os::unix::fs::symlink(&secret, source.join("id_rsa")).unwrap();
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();

    let error = reject(&source, &packs_root);
    assert!(
        matches!(error.failure, ImportFailure::Symlink { .. }),
        "{error:?}"
    );
    assert_uninstalled(&packs_root);
    assert!(!tree_has_bytes(&packs_root, b"SECRET-KEY-MATERIAL"));
}

#[test]
#[cfg(unix)]
fn folder_fifo_is_rejected_within_one_second() {
    let tmp = Tmp::new("fifo");
    let source = tmp.path().join("mascot");
    write_pack(&source, SIT);
    let fifo = source.join("pipe");
    let status = Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(status.success(), "mkfifo failed: {status}");
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let source_thread = source.clone();
    let root_thread = packs_root.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let result = import_pack(&source_thread, &root_thread, CollisionPolicy::Report);
        let _ = tx.send((started.elapsed(), result));
    });
    let (elapsed, result) = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("fifo import did not return");
    assert!(
        elapsed < Duration::from_secs(1),
        "fifo rejection took {elapsed:?}"
    );
    let error = result.expect_err("fifo should be rejected");
    assert!(error.failure.is_fifo(), "{error:?}");
    assert_uninstalled(&packs_root);
}

#[test]
fn folder_over_size_cap_leaves_nothing() {
    let tmp = Tmp::new("size");
    let source = tmp.path().join("mascot");
    fs::create_dir_all(&source).unwrap();
    let big = File::create(source.join("big.bin")).unwrap();
    big.set_len(MAX_UNCOMPRESSED_BYTES + 1).unwrap();
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();

    let error = reject(&source, &packs_root);
    match error.failure {
        ImportFailure::ActualSize { bytes } => assert!(bytes > MAX_UNCOMPRESSED_BYTES),
        other => panic!("expected ActualSize, got {other:?}"),
    }
    assert_uninstalled(&packs_root);
}

#[test]
fn source_mutation_after_copy_does_not_change_installed_copy() {
    let tmp = Tmp::new("race");
    let source = tmp.path().join("mascot");
    write_pack(&source, SIT);
    let sentinel = source.join("shimeji-pause-sentinel.bin");
    fs::write(&sentinel, b"ORIGINAL-BYTES").unwrap();
    let original_actions = fs::read(source.join("actions.xml")).unwrap();
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();
    let pause = tmp.path().join("pause");
    fs::create_dir_all(&pause).unwrap();

    struct EnvGuard;
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // The seam is process-global. Only this test sets it, and the
            // watched path is a file name no other test creates.
            unsafe {
                std::env::remove_var("SHIMEJI_PACKS_PAUSE_AFTER");
                std::env::remove_var("SHIMEJI_PACKS_PAUSE_DIR");
            }
        }
    }
    let _guard = EnvGuard;
    unsafe {
        std::env::set_var("SHIMEJI_PACKS_PAUSE_AFTER", "shimeji-pause-sentinel.bin");
        std::env::set_var("SHIMEJI_PACKS_PAUSE_DIR", &pause);
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let source_thread = source.clone();
    let root_thread = packs_root.clone();
    std::thread::spawn(move || {
        let result = import_pack(&source_thread, &root_thread, CollisionPolicy::Report);
        let _ = tx.send(result);
    });
    let ready = pause.join("ready");
    let started = Instant::now();
    while !ready.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "import never reached the pause point"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    fs::write(&sentinel, b"CORRUPTED-BYTES").unwrap();
    fs::write(source.join("actions.xml"), b"CORRUPTED-XML").unwrap();
    fs::write(pause.join("go"), b"1").unwrap();
    let outcome = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("import did not finish")
        .expect("import should succeed");

    let installed = outcome.path;
    assert_eq!(
        fs::read(installed.join("shimeji-pause-sentinel.bin")).unwrap(),
        b"ORIGINAL-BYTES"
    );
    assert_eq!(
        fs::read(installed.join("actions.xml")).unwrap(),
        original_actions
    );
    assert!(!packs_root.join("staging").exists());
}

#[test]
fn cp932_zip_without_utf8_flag_finds_actions_xml() {
    let actions = actions_xml(SIT);
    let behaviors = behaviors_xml();
    let actions_name = cp932("動作.xml");
    let behaviors_name = cp932("行動.xml");
    let zip_path_bytes = zip_bytes(&[
        Entry {
            name: actions_name,
            method: 0,
            flags: 0,
            uncompressed_size: actions.len() as u32,
            crc: crc32(actions.as_bytes()),
            payload: actions.into_bytes(),
            made_by: 20,
            external: 0,
        },
        Entry {
            name: behaviors_name,
            method: 0,
            flags: 0,
            uncompressed_size: behaviors.len() as u32,
            crc: crc32(behaviors.as_bytes()),
            payload: behaviors.into_bytes(),
            made_by: 20,
            external: 0,
        },
        stored_utf8("img/p.png", common::PNG),
    ]);

    let tmp = Tmp::new("cp932");
    let zip_path = tmp.path().join("mascot.zip");
    fs::write(&zip_path, zip_path_bytes).unwrap();
    let packs_root = tmp.path().join("packs-root");
    let outcome = import_pack(&zip_path, &packs_root, CollisionPolicy::Report)
        .expect("CP932 zip should import");
    assert!(
        !outcome.pack.characters.is_empty(),
        "no characters: {:?}",
        outcome.diagnostics
    );
    let actions_file = &outcome.pack.characters[0].actions_file;
    assert!(
        actions_file.contains("動作.xml"),
        "actions file was {actions_file}"
    );
    assert!(!packs_root.join("staging").exists());
}

fn cp932(text: &str) -> Vec<u8> {
    let (encoded, _, had_errors) = encoding_rs::SHIFT_JIS.encode(text);
    assert!(!had_errors, "{text} is not representable in CP932");
    encoded.into_owned()
}

#[test]
fn jpeg_renamed_to_png_is_a_diagnostic() {
    let tmp = Tmp::new("jpeg");
    let source = tmp.path().join("mascot");
    write_pack(&source, SIT);
    fs::write(
        source.join("img/photo.png"),
        b"\xff\xd8\xff\xe0jpeg-not-png",
    )
    .unwrap();
    let packs_root = tmp.path().join("packs-root");
    let outcome = import_pack(&source, &packs_root, CollisionPolicy::Report)
        .expect("jpeg is a warning, not a rejection");
    assert!(!outcome.pack.characters.is_empty());
    assert!(
        outcome.diagnostics.iter().any(|diag| {
            diag.kind == DiagnosticKind::Other
                && diag.file.contains("photo.png")
                && diag.message.contains("not a PNG")
        }),
        "{:?}",
        outcome.diagnostics
    );
}

#[test]
fn many_large_pngs_trip_the_decode_budget() {
    let png = transparent_png(4096);
    let one = png.len() as u64;
    let actions = actions_xml(SIT);
    let behaviors = behaviors_xml();
    // A valid 4096² blank PNG is tens of kilobytes (deflate's longest match
    // is 258 bytes). 5,000 of them are over the 200 MB copy cap, so that pack
    // would be rejected before the decoder. Take every image that still fits.
    // Two config files also count, and the entry cap is 5,000.
    let overhead = (actions.len() + behaviors.len()) as u64;
    let by_bytes = (MAX_UNCOMPRESSED_BYTES - overhead) / one.max(1);
    let count = usize::try_from(by_bytes).unwrap().min(4_998);
    assert!(
        count >= 1_000,
        "transparent 4096 png is {one} bytes; only {count} fit under the copy cap"
    );
    assert!(overhead + one * count as u64 <= MAX_UNCOMPRESSED_BYTES);

    let tmp = Tmp::new("budget");
    let source = tmp.path().join("mascot");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("actions.xml"), actions).unwrap();
    fs::write(source.join("behaviors.xml"), behaviors).unwrap();
    for index in 0..count {
        fs::write(source.join(format!("f{index:04}.png")), &png).unwrap();
    }
    let packs_root = tmp.path().join("packs-root");
    let error = reject(&source, &packs_root);
    match error.failure {
        ImportFailure::DecodeBudget {
            decoded_images,
            used_bytes,
            next_bytes,
            ..
        } => {
            assert!(
                decoded_images.saturating_mul(2) < count,
                "decoded {decoded_images} of {count}"
            );
            assert!(decoded_images <= 8, "decoded {decoded_images}");
            assert!(used_bytes <= MAX_DECODED_BYTES);
            assert!(used_bytes + next_bytes > MAX_DECODED_BYTES);
        }
        other => panic!("expected DecodeBudget, got {other:?}"),
    }
    assert_uninstalled(&packs_root);
}

fn transparent_png(edge: u32) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    // PNG IDAT is a zlib stream of scanlines, each starting with filter 0.
    // A blank image compresses to a few kilobytes, so thousands of copies
    // stay under the 200 MB copy cap and reach the decode budget.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    let row = vec![0u8; edge as usize * 4];
    for _ in 0..edge {
        encoder.write_all(&[0]).unwrap();
        encoder.write_all(&row).unwrap();
    }
    let idat = encoder.finish().unwrap();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&edge.to_be_bytes());
    ihdr.extend_from_slice(&edge.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &idat);
    png_chunk(&mut out, b"IEND", b"");
    out
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut hashed = Vec::with_capacity(4 + data.len());
    hashed.extend_from_slice(kind);
    hashed.extend_from_slice(data);
    out.extend_from_slice(&crc32(&hashed).to_be_bytes());
}

#[test]
fn replace_is_atomic_and_keep_both_is_the_callers_choice() {
    let tmp = Tmp::new("swap");
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();
    let source = tmp.path().join("mascot");
    write_pack(&source, SIT);
    fs::write(source.join("marker.txt"), b"one").unwrap();
    fs::write(source.join("old-only.txt"), b"old").unwrap();

    let first = import_pack(&source, &packs_root, CollisionPolicy::Report).unwrap();
    assert_eq!(first.id, "mascot");
    assert!(first.collision.is_none());

    fs::write(source.join("marker.txt"), b"two").unwrap();
    fs::remove_file(source.join("old-only.txt")).unwrap();
    fs::write(source.join("new-only.txt"), b"new").unwrap();
    let reported = reject(&source, &packs_root);
    assert!(
        matches!(reported.failure, ImportFailure::Collision { .. }),
        "{reported:?}"
    );
    assert_eq!(
        fs::read(packs_root.join("packs/mascot/marker.txt")).unwrap(),
        b"one"
    );
    assert!(packs_root.join("packs/mascot/old-only.txt").is_file());

    let replaced = import_pack(&source, &packs_root, CollisionPolicy::Replace).unwrap();
    assert_eq!(replaced.id, "mascot");
    let collision = replaced.collision.expect("replace reports the collision");
    assert_eq!(collision.existing_id, "mascot");
    assert_eq!(collision.policy, CollisionPolicy::Replace);
    assert_eq!(fs::read(replaced.path.join("marker.txt")).unwrap(), b"two");
    assert!(replaced.path.join("new-only.txt").is_file());
    assert!(!replaced.path.join("old-only.txt").exists());
    let leftovers: Vec<_> = fs::read_dir(packs_root.join("packs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.') || name.contains("replacing"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert!(!packs_root.join("staging").exists());

    fs::write(source.join("marker.txt"), b"three").unwrap();
    let kept = import_pack(&source, &packs_root, CollisionPolicy::KeepBoth).unwrap();
    assert_eq!(kept.id, "mascot-2");
    let collision = kept.collision.expect("keep-both reports the collision");
    assert_eq!(collision.existing_id, "mascot");
    assert_eq!(collision.policy, CollisionPolicy::KeepBoth);
    assert_eq!(fs::read(kept.path.join("marker.txt")).unwrap(), b"three");
    assert_eq!(
        fs::read(packs_root.join("packs/mascot/marker.txt")).unwrap(),
        b"two"
    );
    assert!(!packs_root.join("staging").exists());
}

#[test]
#[cfg(unix)]
fn load_dir_does_not_treat_a_symlink_as_a_config_file() {
    let tmp = Tmp::new("cfg-link");
    let outside = tmp.path().join("outside-actions.xml");
    fs::write(
        &outside,
        actions_xml(r#"    <Action Name="FromOutside" Type="Stay"/>"#),
    )
    .unwrap();
    let pack = tmp.path().join("linked");
    fs::create_dir_all(&pack).unwrap();
    fs::write(pack.join("behaviors.xml"), behaviors_xml()).unwrap();
    std::os::unix::fs::symlink(&outside, pack.join("actions.xml")).unwrap();

    let outcome = load_dir(&pack).unwrap();
    assert!(
        outcome.pack.characters.is_empty(),
        "symlink config was followed: {:?}",
        outcome.pack.characters
    );
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|diag| diag.kind == DiagnosticKind::MissingConfig),
        "{:?}",
        outcome.diagnostics
    );
}

#[test]
#[cfg(unix)]
fn load_dir_does_not_index_a_symlinked_image() {
    let tmp = Tmp::new("img-link");
    let outside = tmp.path().join("real.png");
    fs::write(&outside, common::PNG).unwrap();
    let pack = tmp.path().join("linked");
    let body = r#"    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>"#;
    fs::create_dir_all(pack.join("img")).unwrap();
    fs::write(pack.join("actions.xml"), actions_xml(body)).unwrap();
    fs::write(pack.join("behaviors.xml"), behaviors_xml()).unwrap();
    std::os::unix::fs::symlink(&outside, pack.join("img/p.png")).unwrap();

    let outcome = load_dir(&pack).unwrap();
    assert!(
        outcome
            .diagnostics
            .iter()
            .any(|diag| diag.kind == DiagnosticKind::MissingImage && diag.message.contains("p.png")),
        "{:?}",
        outcome.diagnostics
    );
}

#[test]
fn load_dir_does_not_index_past_the_depth_limit() {
    let tmp = Tmp::new("depth");
    let pack = tmp.path().join("deep");
    fs::create_dir_all(&pack).unwrap();
    let mut leaf = pack.clone();
    let mut ok_rel = String::new();
    let mut leaf_rel = String::new();
    for index in 1..=MAX_DEPTH {
        let name = format!("d{index}");
        leaf.push(&name);
        if index < MAX_DEPTH {
            if !ok_rel.is_empty() {
                ok_rel.push('/');
            }
            ok_rel.push_str(&name);
        }
        if !leaf_rel.is_empty() {
            leaf_rel.push('/');
        }
        leaf_rel.push_str(&name);
    }
    fs::create_dir_all(&leaf).unwrap();
    fs::write(leaf.join("leaf.png"), common::PNG).unwrap();
    let mut ok = pack.clone();
    for index in 1..MAX_DEPTH {
        ok.push(format!("d{index}"));
    }
    fs::write(ok.join("ok.png"), common::PNG).unwrap();
    ok_rel.push_str("/ok.png");
    leaf_rel.push_str("/leaf.png");
    let body = format!(
        r#"    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/{ok_rel}" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
        <Pose Image="/{leaf_rel}" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>"#
    );
    fs::write(pack.join("actions.xml"), actions_xml(&body)).unwrap();
    fs::write(pack.join("behaviors.xml"), behaviors_xml()).unwrap();

    let outcome = load_dir(&pack).unwrap();
    let missing: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|diag| diag.kind == DiagnosticKind::MissingImage)
        .map(|diag| diag.message.clone())
        .collect();
    assert!(
        missing.iter().any(|message| message.contains("leaf.png")),
        "deep file was indexed: {missing:?}"
    );
    assert!(
        missing.iter().all(|message| !message.contains("ok.png")),
        "file at depth {MAX_DEPTH} was dropped: {missing:?}"
    );
}

#[test]
fn bundled_shime_imports_as_folder_and_zip() {
    let shime = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime");
    let tmp = Tmp::new("shime");
    let folder_root = tmp.path().join("folder");
    let outcome = import_pack(&shime, &folder_root, CollisionPolicy::Report)
        .expect("shime folder should import");
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    assert!(!outcome.images.is_empty());
    assert!(!outcome.pack.characters.is_empty());
    assert!(!folder_root.join("staging").exists());

    let zip_path = tmp.path().join("shime.zip");
    fs::write(&zip_path, zip_tree(&shime)).unwrap();
    let zip_root = tmp.path().join("zip");
    let zipped = import_pack(&zip_path, &zip_root, CollisionPolicy::Report)
        .expect("shime zip should import");
    assert!(zipped.diagnostics.is_empty(), "{:#?}", zipped.diagnostics);
    assert_eq!(zipped.images.len(), outcome.images.len());
    assert_eq!(zipped.pack.characters.len(), outcome.pack.characters.len());
    assert!(!zip_root.join("staging").exists());
}

#[test]
fn duplicate_zip_names_that_differ_by_case_name_both_entries() {
    let tmp = Tmp::new("case-dup");
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();
    let zip_path = tmp.path().join("case.zip");
    fs::write(
        &zip_path,
        zip_bytes(&[
            stored_utf8("img/Q.png", b"CASE-Q"),
            stored_utf8("img/q.png", b"CASE-q"),
        ]),
    )
    .unwrap();

    let error = reject(&zip_path, &packs_root);
    match &error.failure {
        ImportFailure::DuplicateEntry { first, second } => {
            assert_eq!(first, "img/Q.png");
            assert_eq!(second, "img/q.png");
        }
        other => panic!("expected DuplicateEntry, got {other:?}"),
    }
    // Central-directory check, not create_new's "File exists (os error 17)".
    // The same names collide after case-folding on a case-sensitive disk.
    assert_eq!(
        error.to_string(),
        "duplicate entry after normalisation: img/Q.png and img/q.png"
    );
    assert_uninstalled(&packs_root);
    assert!(!tree_has_bytes(&packs_root, b"CASE-Q"));
    assert!(!tree_has_bytes(&packs_root, b"CASE-q"));
}

#[test]
fn duplicate_zip_names_that_differ_by_nfc_name_both_entries() {
    let nfc = "img/\u{00e9}.png";
    let nfd = "img/e\u{0301}.png";
    assert_ne!(nfc, nfd);
    let tmp = Tmp::new("nfc-dup");
    let packs_root = tmp.path().join("packs-root");
    fs::create_dir_all(&packs_root).unwrap();
    let zip_path = tmp.path().join("nfc.zip");
    fs::write(
        &zip_path,
        zip_bytes(&[stored_utf8(nfc, b"NFC-ONE"), stored_utf8(nfd, b"NFD-TWO")]),
    )
    .unwrap();

    let error = reject(&zip_path, &packs_root);
    match &error.failure {
        ImportFailure::DuplicateEntry { first, second } => {
            assert_eq!(first, nfc);
            assert_eq!(second, nfd);
        }
        other => panic!("expected DuplicateEntry, got {other:?}"),
    }
    assert_eq!(
        error.to_string(),
        format!("duplicate entry after normalisation: {nfc} and {nfd}")
    );
    assert_uninstalled(&packs_root);
    assert!(!tree_has_bytes(&packs_root, b"NFC-ONE"));
    assert!(!tree_has_bytes(&packs_root, b"NFD-TWO"));
}

fn zip_tree(root: &Path) -> Vec<u8> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).unwrap();
            assert!(!meta.file_type().is_symlink(), "{}", path.display());
            if meta.is_dir() {
                dirs.push(path);
            } else if meta.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    let entries = files
        .iter()
        .map(|path| {
            let rel = path.strip_prefix(root).unwrap();
            let name = rel
                .components()
                .map(|component| component.as_os_str().to_str().unwrap())
                .collect::<Vec<_>>()
                .join("/");
            let data = fs::read(path).unwrap();
            stored_utf8(&name, &data)
        })
        .collect::<Vec<_>>();
    zip_bytes(&entries)
}
