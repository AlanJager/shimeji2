//! Placement against the measured shime templates. Synthetic sprites only.

use shimeji_forge::{RgbaImage, ScaleMethod, SheetFrame, bundled_shime_img, measure_dir, place};

#[test]
fn a_sprite_twenty_percent_smaller_meets_the_contact_edge() {
    let templates = measure_dir(&bundled_shime_img()).expect("shime img");
    let stand = templates.get(1).expect("frame 1");
    let stand_h = (stand.bbox.h * 4) / 5;
    let stand_w = (stand.bbox.w * 4) / 5;
    assert!(stand_h > stand_w, "the standing fixture should be upright");

    let mut frames = vec![
        SheetFrame {
            index: 1,
            image: RgbaImage::solid(stand_w, stand_h, [180, 40, 60, 255]),
        },
        // Shorter than frame 1, so the sheet scale stays on the tallest upright frame.
        SheetFrame {
            index: 2,
            image: RgbaImage::solid(40, 80, [180, 40, 60, 255]),
        },
    ];
    for index in 23u16..=25 {
        let template = templates.get(index).expect("ceiling");
        frames.push(SheetFrame {
            index,
            image: RgbaImage::solid(
                (template.bbox.w * 4 / 5).max(1),
                (template.bbox.h * 4 / 5).max(1),
                [180, 40, 60, 255],
            ),
        });
    }

    let placed = place(&frames, &templates).expect("place");
    assert_eq!(placed.method, ScaleMethod::TallestUpright);
    assert_eq!(placed.reference, 1);
    let expect_scale = stand.bbox.h as f32 / stand_h as f32;
    assert!((placed.scale - expect_scale).abs() < 1e-4);

    let feet = placed.frame(1).unwrap().image.feet_y().unwrap();
    assert!(
        (feet - 128).abs() <= 2,
        "standing feet at {feet}, want 128 ± 2"
    );

    let walk = placed.frame(2).unwrap();
    let walk_feet = walk.image.feet_y().unwrap();
    assert!((walk_feet - 128).abs() <= 2, "walk feet at {walk_feet}");
    let scaled_h = ((80.0 * placed.scale).round()) as i32;
    let got_h = walk.content_height as i32;
    assert!(
        (got_h - scaled_h).abs() <= 1,
        "frame 2 used the sheet scale: height {got_h}, scaled {scaled_h}"
    );

    for index in 23u16..=25 {
        let frame = placed.frame(index).unwrap();
        let top = frame.image.top_y().unwrap();
        let contact = templates.get(index).unwrap().contact_at;
        assert!(
            (top - contact).abs() <= 2,
            "frame {index} top {top}, template edge {contact}"
        );
    }
}
