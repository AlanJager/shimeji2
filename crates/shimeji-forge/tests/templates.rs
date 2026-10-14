//! Templates measured from the bundled shime, and the generated mannequin guides.

use shimeji_forge::{
    Edge, OPAQUE, bundled_shime_img, decode_png, load_templates, mannequin, measure_dir,
    pose_guides_dir, render_poses_toml, templates_path,
};

#[test]
fn measured_templates_are_stable_and_ceiling_frames_contact_the_top() {
    let once = measure_dir(&bundled_shime_img()).expect("measure");
    let twice = measure_dir(&bundled_shime_img()).expect("measure again");
    assert_eq!(once, twice, "measuring twice changes nothing");
    assert_eq!(once.frames.len(), 46);

    let file = load_templates(&templates_path()).expect("templates.toml");
    assert_eq!(once, file, "templates.toml is the measurement");

    for index in 23u16..=25 {
        let frame = once.get(index).unwrap();
        assert_eq!(frame.edge, Edge::Top, "frame {index}");
        assert_eq!((frame.anchor_x, frame.anchor_y), (64, 48));
    }
    for frame in &once.frames {
        if (23..=25).contains(&frame.index) {
            continue;
        }
        assert_ne!(
            frame.edge,
            Edge::Top,
            "frame {} is not a ceiling pose",
            frame.index
        );
    }
    let stand = once.get(1).unwrap();
    assert_eq!(stand.edge, Edge::Bottom);
    assert_eq!(stand.contact_at, 128);
    assert_eq!((stand.anchor_x, stand.anchor_y), (64, 128));
}

#[test]
fn pose_guides_are_flat_fills_of_the_shime_silhouettes() {
    let shime = bundled_shime_img();
    let guides = pose_guides_dir();
    let poses = std::fs::read_to_string(guides.join("poses.toml")).expect("poses.toml");
    assert_eq!(poses, render_poses_toml());
    for index in 1u16..=46 {
        let src = decode_png(&shime.join(format!("shime{index}.png"))).unwrap();
        let guide = decode_png(&guides.join(format!("shime{index}.png"))).unwrap();
        assert_eq!((guide.width, guide.height), (src.width, src.height));
        let fresh = mannequin(&src);
        assert_eq!(guide, fresh, "shime{index} guide drifted");
        for y in 0..src.height {
            for x in 0..src.width {
                let ink = src.get(x, y)[3] >= OPAQUE;
                let px = guide.get(x, y);
                if ink {
                    assert_eq!(px[3], 255);
                    assert!(px[0] == 186 || px[0] == 90, "flat fill only");
                } else {
                    assert_eq!(px[3], 0);
                }
            }
        }
    }
}
