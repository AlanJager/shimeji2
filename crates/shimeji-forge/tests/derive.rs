//! Split frames are two offset copies. Carry frames lose the window rectangle.

use std::fs;
use std::path::PathBuf;

use shimeji_forge::raster::RgbaImage;
use shimeji_forge::{clip_frame, derive_pack, scaled_offset, split_frame};

fn repo_img() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime/img")
}

fn edges(image: &RgbaImage) -> (u32, u32) {
    let mut left = image.width;
    let mut right = 0u32;
    let mut any = false;
    for y in 0..image.height {
        for x in 0..image.width {
            if image.get(x, y)[3] > 0 {
                any = true;
                left = left.min(x);
                right = right.max(x);
            }
        }
    }
    assert!(any, "image has no ink");
    (left, right)
}

fn scratch() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("shimeji-u16-derive-{}-{nanos}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("img")).unwrap();
    dir
}

#[test]
fn split_frames_are_two_offset_copies_and_match_the_bundled_shime_edges() {
    let img = repo_img();
    let frame1 = shimeji_forge::decode_png(&img.join("shime1.png")).unwrap();
    assert_eq!(edges(&frame1), (20, 110));
    for (frame, offset) in [(42u16, 4i32), (43, 8), (44, 12), (45, 16), (46, 20)] {
        assert_eq!(scaled_offset(offset, 128), offset);
        let made = split_frame(&frame1, offset);
        let expect_left = 20 - offset as u32;
        let expect_right = (110 + offset as u32).min(127);
        assert_eq!(
            edges(&made),
            (expect_left, expect_right),
            "composite shime{frame}"
        );
        // The drawn shime45 stops two pixels short of a full +16 on the right.
        // shime46 is clipped by the 128-wide cell (110+20 = 130).
        let bundled_right = match offset {
            16 => 124,
            20 => 127,
            other => 110 + other as u32,
        };
        let bundled = shimeji_forge::decode_png(&img.join(format!("shime{frame}.png"))).unwrap();
        assert_eq!(
            edges(&bundled),
            (expect_left, bundled_right),
            "bundled shime{frame}"
        );
    }

    let mut source = RgbaImage::new(128, 128);
    source.set(20, 10, [255, 0, 0, 255]);
    source.set(110, 10, [0, 0, 255, 255]);
    let made = split_frame(&source, 4);
    assert_eq!(made.get(16, 10), [255, 0, 0, 255], "left copy");
    assert_eq!(made.get(24, 10), [255, 0, 0, 255], "right copy");
    assert_eq!(made.get(114, 10), [0, 0, 255, 255], "right edge copy");
    assert_eq!(
        made.get(106, 10),
        [0, 0, 255, 255],
        "left edge of the right copy"
    );
}

#[test]
fn clipped_carry_frames_have_no_pixels_past_the_clip_line() {
    let mut image = RgbaImage::solid(128, 128, [10, 20, 30, 255]);
    let (x, y) = clip_frame(&mut image);
    assert_eq!((x, y), (64, 64));
    for row in 0..64 {
        for col in 0..64 {
            assert_eq!(image.get(col, row)[3], 0, "pixel {col},{row}");
        }
    }
    assert_eq!(image.get(64, 0)[3], 255);
    assert_eq!(image.get(0, 64)[3], 255);
    assert_eq!(image.get(63, 63)[3], 0);

    let mut tall = RgbaImage::solid(128, 135, [1, 2, 3, 255]);
    assert_eq!(clip_frame(&mut tall), (64, 67));
    assert_eq!(tall.get(0, 66)[3], 0);
    assert_eq!(tall.get(0, 67)[3], 255);
}

#[test]
fn derive_pack_writes_splits_clips_and_derive_json() {
    let root = scratch();
    let mut stand = RgbaImage::new(128, 128);
    stand.fill_rect(20, 1, 91, 127, [9, 9, 9, 255]);
    shimeji_forge::save_png(&root.join("img/shime1.png"), &stand).unwrap();
    for index in [34, 35, 36] {
        let image = RgbaImage::solid(128, 128, [4, 5, 6, 255]);
        shimeji_forge::save_png(&root.join(format!("img/shime{index}.png")), &image).unwrap();
    }
    let report = derive_pack(&root).unwrap();
    assert_eq!(
        report
            .offsets
            .iter()
            .map(|row| (row.frame, row.dx))
            .collect::<Vec<_>>(),
        vec![(42, 4), (43, 8), (44, 12), (45, 16), (46, 20)]
    );
    assert_eq!(report.clips.len(), 3);
    let split = shimeji_forge::decode_png(&root.join("img/shime46.png")).unwrap();
    assert_eq!(edges(&split), (0, 127));
    let clipped = shimeji_forge::decode_png(&root.join("img/shime34.png")).unwrap();
    assert_eq!(clipped.get(63, 63)[3], 0);
    assert_eq!(clipped.get(64, 0)[3], 255);
    let body: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("derive.json")).unwrap()).unwrap();
    assert_eq!(body["offsets"][4]["dx"], 20);
    assert_eq!(body["clips"][0]["x"], 64);
    let _ = fs::remove_dir_all(&root);
}
