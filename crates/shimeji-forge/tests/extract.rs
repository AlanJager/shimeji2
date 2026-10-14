//! Synthetic sheets only. No network.

use shimeji_forge::{
    KeyKind, RgbaImage, extract, extract_grid, frame_contains_color, frames_inside_cells,
    fringe_ratio,
};

const GREEN: [u8; 4] = [0, 255, 0, 255];
const INK: [u8; 4] = [180, 40, 60, 255];
const FRINGE: [u8; 4] = [0, 220, 0, 255];

#[test]
fn five_sprites_on_green_with_a_two_pixel_fringe() {
    let cell = 64u32;
    let cols = 5usize;
    let mut sheet = RgbaImage::solid(cell * cols as u32, cell, GREEN);
    for i in 0..cols {
        let x = i as u32 * cell;
        // A 2 px fringe around a sprite that itself sits inside the slot.
        sheet.fill_rect(x + 6, 6, cell - 12, cell - 12, FRINGE);
        sheet.fill_rect(x + 8, 8, cell - 16, cell - 16, INK);
    }
    let extracted = extract(&sheet, cols).expect("extract");
    assert_eq!(extracted.frames.len(), 5);
    assert!(
        extracted
            .frames
            .iter()
            .all(|frame| frame.image.opaque_count() > 100),
        "each slot keeps its sprite"
    );
    assert!(
        frames_inside_cells(&extracted),
        "no crop sits outside its slot"
    );
    assert!(
        extracted.fringe_ratio < 0.01,
        "fringe ratio {}",
        extracted.fringe_ratio
    );
    for frame in &extracted.frames {
        let ratio = fringe_ratio(&frame.image, extracted.key_rgb());
        assert!(ratio < 0.01, "slot {} fringe {ratio}", frame.slot);
        assert!(
            !frame_contains_color(&frame.image, |px| px[1]
                > px[0].max(px[2]).saturating_add(16)),
            "green fringe survived in slot {}",
            frame.slot
        );
    }
}

#[test]
fn background_is_detected_from_the_corners_when_the_key_is_not_pure_green() {
    let key = [0x10u8, 0xF0, 0x10, 255];
    let mut sheet = RgbaImage::solid(80, 48, key);
    sheet.fill_rect(24, 12, 20, 24, INK);
    let extracted = extract(&sheet, 1).expect("extract");
    assert_eq!(extracted.key, KeyKind::Chroma([0x10, 0xF0, 0x10]));
    assert_eq!(extracted.frames.len(), 1);
    assert!(extracted.frames[0].image.opaque_count() > 50);
    assert!(extracted.fringe_ratio < 0.01);
}

#[test]
fn touching_poses_stay_in_equal_columns() {
    // Two sprites share an edge at the midpoint. A connected-component split of
    // the merged blob would cut near x=40 and put red ink into the right frame.
    let mut sheet = RgbaImage::solid(100, 40, GREEN);
    sheet.fill_rect(10, 8, 40, 24, [220, 20, 40, 255]);
    sheet.fill_rect(50, 8, 30, 24, [20, 20, 220, 255]);
    let extracted = extract(&sheet, 2).expect("extract");
    assert_eq!(extracted.frames.len(), 2);
    assert!(frames_inside_cells(&extracted));
    let left = &extracted.frames[0].image;
    let right = &extracted.frames[1].image;
    assert!(frame_contains_color(left, |px| px[0] > 180 && px[2] < 80));
    assert!(!frame_contains_color(left, |px| px[2] > 180 && px[0] < 80));
    assert!(frame_contains_color(right, |px| px[2] > 180 && px[0] < 80));
    assert!(!frame_contains_color(right, |px| px[0] > 180 && px[2] < 80));
    assert!(extracted.frames[0].origin_x + left.width <= 50);
    assert!(extracted.frames[1].origin_x >= 50);
}

#[test]
fn detached_specks_are_removed() {
    let mut sheet = RgbaImage::solid(80, 80, GREEN);
    sheet.fill_rect(10, 10, 40, 40, INK);
    sheet.fill_rect(60, 60, 2, 2, INK);
    let extracted = extract(&sheet, 1).expect("extract");
    let frame = &extracted.frames[0];
    assert!(frame.image.opaque_count() > 1000);
    // The speck sat at the bottom-right of the sheet. It must not survive in the crop.
    assert!(frame.origin_x < 20);
    assert!(frame.origin_y < 20);
    assert!(frame.image.width < 50);
    assert!(frame.image.height < 50);
}

#[test]
fn a_grid_splits_rows_as_well_as_columns() {
    let mut sheet = RgbaImage::solid(40, 40, GREEN);
    sheet.fill_rect(4, 4, 10, 10, INK);
    sheet.fill_rect(24, 24, 10, 10, INK);
    let extracted = extract_grid(&sheet, 2, 2).expect("grid");
    assert_eq!(extracted.frames.len(), 4);
    assert!(extracted.frames[0].image.opaque_count() > 0);
    assert_eq!(extracted.frames[1].image.opaque_count(), 0);
    assert_eq!(extracted.frames[2].image.opaque_count(), 0);
    assert!(extracted.frames[3].image.opaque_count() > 0);
}
