//! Classic export writes `shime{N}.png` and shifts every use of an anchor together.

use std::fs;
use std::path::PathBuf;

use shimeji_packs::{export_classic, write_standard_pack};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("shimeji-u13-export-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    (
        u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
        u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
    )
}

fn anchors(xml: &str, image: &str) -> Vec<(i32, i32)> {
    let mut found = Vec::new();
    for line in xml.lines() {
        if !line.contains(image) {
            continue;
        }
        let key = "基準座標=\"";
        let start = line.find(key).unwrap() + key.len();
        let end = line[start..].find('"').unwrap();
        let (x, y) = line[start..start + end].split_once(',').unwrap();
        found.push((x.parse().unwrap(), y.parse().unwrap()));
    }
    found
}

#[test]
fn classic_export_writes_shime_pngs_and_keeps_per_use_anchor_gaps() {
    let root = repo();
    let work = scratch();
    let native = work.join("native");
    write_standard_pack(&root.join("assets/packs/shime"), &native).unwrap();

    let mut body: serde_json::Value =
        serde_json::from_slice(&fs::read(native.join("pack.json")).unwrap()).unwrap();
    body["poses"] = serde_json::json!({
        "pinched-c": {"anchor": [70, 128]},
        "bounce-b": {"anchor": [64, 120]}
    });
    fs::write(
        native.join("pack.json"),
        serde_json::to_vec_pretty(&body).unwrap(),
    )
    .unwrap();

    let dest = work.join("out");
    export_classic(&native, &dest).unwrap();
    for index in 1..=46 {
        let bytes = fs::read(dest.join(format!("img/shime{index}.png"))).unwrap();
        assert_eq!(png_size(&bytes), (128, 128), "shime{index}.png");
    }
    let xml = fs::read_to_string(dest.join("conf/動作.xml")).unwrap();
    assert_eq!(
        anchors(&xml, "/shime9.png"),
        vec![(70, 128), (38, 128), (38, 128), (38, 128)]
    );
    assert_eq!(
        anchors(&xml, "/shime19.png"),
        vec![(64, 120), (64, 120), (64, 96)]
    );
    let behaviors = fs::read(dest.join("conf/行動.xml")).unwrap();
    assert_eq!(
        behaviors,
        fs::read(root.join("assets/packs/shime/conf/行動.xml")).unwrap()
    );
    let _ = fs::remove_dir_all(&work);
}
