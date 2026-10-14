//! The bundled shime pack matches the SHA-256 list copied from upstream.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn pack_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime")
}

fn expected_relative_paths() -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    paths.insert("conf/動作.xml".to_string());
    paths.insert("conf/行動.xml".to_string());
    for index in 1..=46 {
        paths.insert(format!("img/shime{index}.png"));
    }
    paths
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn file_names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("read entry in {}: {error}", dir.display()))
                .file_name()
                .into_string()
                .unwrap_or_else(|name| panic!("non-utf8 file name in {}: {name:?}", dir.display()))
        })
        .collect()
}

#[test]
fn bundled_shime_files_match_checksums() {
    let root = pack_root();
    let checksum_path = root.join("CHECKSUMS.sha256");
    let text = fs::read_to_string(&checksum_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", checksum_path.display()));

    let mut recorded = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_number = index + 1;
        let (hash, relative) = line.split_once("  ").unwrap_or_else(|| {
            panic!("line {line_number}: expected \"<sha256>  <path>\", got {line:?}")
        });
        assert_eq!(hash.len(), 64, "line {line_number}: hash length");
        assert!(
            hash.chars().all(|character| character.is_ascii_hexdigit()),
            "line {line_number}: hash is not hex"
        );
        assert!(
            recorded.insert(relative.to_string()),
            "duplicate checksum for {relative}"
        );

        let bytes = fs::read(root.join(relative))
            .unwrap_or_else(|error| panic!("read {relative}: {error}"));
        let actual = sha256_hex(&bytes);
        assert_eq!(actual, hash.to_ascii_lowercase(), "{relative}");
    }

    assert_eq!(
        recorded,
        expected_relative_paths(),
        "checksum list must be exactly the 46 PNGs and 2 XML files"
    );

    let png_names = (1..=46)
        .map(|index| format!("shime{index}.png"))
        .collect::<BTreeSet<_>>();
    assert_eq!(file_names(&root.join("img")), png_names);
    assert_eq!(
        file_names(&root.join("conf")),
        BTreeSet::from(["動作.xml".to_string(), "行動.xml".to_string()])
    );
}
