use shimeji_macos::platform::hit_test::{AlphaMask, opaque_at};

#[test]
fn clear_cells_and_missing_pixels_never_hit() {
    let blank = AlphaMask::from_rgba(&[255, 255, 255, 0], 2, 2);
    assert!(!blank.has_pixels());
    assert!(!blank.hit(0.5, 0.5, 1.0, false));
    let empty = AlphaMask::from_rgba(&[], 0, 0);
    assert!(!empty.has_pixels());
    let short = AlphaMask::from_rgba(&[0, 0, 0, 255], 2, 2);
    assert!(short.hit(0.5, 0.5, 1.0, false));
    assert!(!short.hit(1.5, 1.5, 1.0, false));
    assert!(!opaque_at(&[], u32::MAX, u32::MAX, i32::MAX, i32::MAX));
}

#[test]
fn alpha_lookup_tracks_mirroring_and_scale_about_the_image_centre() {
    let mask = AlphaMask::from_rgba(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 2, 2);
    assert!(mask.hit(0.5, 0.5, 1.0, false));
    assert!(!mask.hit(1.5, 0.5, 1.0, false));
    assert!(mask.hit(1.5, 0.5, 1.0, true));
    assert!(!mask.hit(0.5, 0.5, 1.0, true));
    assert!(mask.hit(-0.5, -0.5, 2.0, false));
    assert!(mask.hit(2.5, -0.5, 2.0, true));
    assert!(mask.hit(0.75, 0.75, 0.5, false));
    assert!(!mask.hit(0.25, 0.75, 0.5, false));
    for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(!mask.hit(0.5, 0.5, scale, false));
    }
    assert!(!mask.hit(f64::NAN, 0.5, 1.0, false));
    assert!(!mask.hit(2.0, 0.5, 1.0, false));
}

#[test]
fn a_different_sized_facing_image_maps_to_the_actual_pose_cell() {
    let mask = AlphaMask::from_rgba(&[0, 0, 0, 255, 0, 0, 0, 0], 2, 1);
    assert!(mask.hit_in_cell((1.0, 1.0), (8, 4), 1.0, false));
    assert!(!mask.hit_in_cell((7.0, 1.0), (8, 4), 1.0, false));
    assert!(mask.hit_in_cell((7.0, 1.0), (8, 4), 1.0, true));
    assert!(!mask.hit_in_cell((0.0, 0.0), (0, 4), 1.0, false));
}
