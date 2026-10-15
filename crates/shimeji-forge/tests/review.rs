//! The contact sheet labels all 46 cells. GIF delays are the pose lengths.

use std::fs;
use std::path::PathBuf;

use shimeji_forge::raster::RgbaImage;
use shimeji_forge::{decode_png, read_all_labels, review, save_png};

fn scratch() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("shimeji-u16-review-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("pack/img")).unwrap();
    fs::create_dir_all(dir.join("pack/conf")).unwrap();
    dir
}

fn encode_frame(dir: &std::path::Path, index: u16) {
    let image = RgbaImage::solid(32, 32, [20, 40, 60, 255]);
    save_png(&dir.join(format!("shime{index}.png")), &image).unwrap();
}

fn gif_delays(bytes: &[u8]) -> Vec<u16> {
    let options = gif::DecodeOptions::new();
    let mut decoder = options.read_info(std::io::Cursor::new(bytes)).unwrap();
    let mut delays = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        delays.push(frame.delay);
    }
    delays
}

#[test]
fn the_contact_sheet_labels_every_cell_and_gifs_use_pose_durations() {
    let root = scratch();
    let pack = root.join("pack");
    for index in 1..=46 {
        encode_frame(&pack.join("img"), index);
    }
    fs::write(
        pack.join("conf/動作.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<マスコット>
  <動作リスト>
    <動作 名前="walk" 種類="移動" 枠="地面">
      <アニメーション>
        <ポーズ 画像="/shime1.png" 基準座標="16,32" 移動速度="-1,0" 長さ="6"/>
        <ポーズ 画像="/shime2.png" 基準座標="16,32" 移動速度="-1,0" 長さ="2"/>
      </アニメーション>
    </動作>
    <動作 名前="pause" 種類="静止" 枠="地面">
      <アニメーション>
        <ポーズ 画像="/shime3.png" 基準座標="16,32" 移動速度="0,0" 長さ="0"/>
      </アニメーション>
    </動作>
  </動作リスト>
</マスコット>
"#,
    )
    .unwrap();
    fs::write(
        pack.join("conf/行動.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<マスコット>
  <行動リスト>
    <行動 名前="stand" 動作="walk" 頻度="1"/>
  </行動リスト>
</マスコット>
"#,
    )
    .unwrap();

    let out = root.join("review");
    let written = review(&pack, &out).unwrap();
    let sheet = decode_png(&written.sheet).unwrap();
    assert_eq!(read_all_labels(&sheet), (1..=46).collect::<Vec<_>>());
    // Glyph "1" is 010 on the top row: the centre pixel is black, the left is white.
    assert_eq!(sheet.get(4, 2), [0, 0, 0, 255]);
    assert_eq!(sheet.get(2, 2), [255, 255, 255, 255]);
    // Cell 46 is column 5, row 5 of a 32px grid. Glyph "4" starts 101.
    assert_eq!(sheet.get(162, 162), [0, 0, 0, 255]);
    assert_eq!(sheet.get(164, 162), [255, 255, 255, 255]);

    let walk = fs::read(out.join("walk.gif")).unwrap();
    assert_eq!(gif_delays(&walk), vec![24, 8]);
    let pause = fs::read(out.join("pause.gif")).unwrap();
    assert_eq!(gif_delays(&pause), vec![1]);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn gifs_open_images_stored_under_nfd_names() {
    let root = scratch();
    let pack = root.join("pack");
    // The XML spells both names in NFC; the disk keeps them in NFD.
    let set = pack.join("img/Cafe\u{0301}");
    fs::create_dir_all(&set).unwrap();
    let image = RgbaImage::solid(24, 16, [20, 40, 60, 255]);
    save_png(&set.join("cafe\u{0301}.png"), &image).unwrap();
    fs::create_dir_all(set.join("conf")).unwrap();
    let nfc_name = "caf\u{00e9}.png";
    fs::write(
        set.join("conf/actions.xml"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <ActionList>
    <Action Name="sit" Type="Stay" BorderType="Floor">
      <Animation>
        <Pose Image="/{nfc_name}" ImageAnchor="12,16" Velocity="0,0" Duration="4"/>
      </Animation>
    </Action>
  </ActionList>
</Mascot>
"#
        ),
    )
    .unwrap();
    fs::write(
        set.join("conf/behaviors.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Mascot>
  <BehaviorList>
    <Behavior Name="sit" Action="sit" Frequency="1"/>
  </BehaviorList>
</Mascot>
"#,
    )
    .unwrap();

    let out = root.join("review");
    review(&pack, &out).unwrap();
    let bytes = fs::read(out.join("sit.gif")).unwrap();
    let decoder = gif::DecodeOptions::new()
        .read_info(std::io::Cursor::new(bytes))
        .unwrap();
    // A pose whose file cannot be opened becomes a 1×1 frame.
    assert_eq!((decoder.width(), decoder.height()), (24, 16));
    let _ = fs::remove_dir_all(&root);
}
