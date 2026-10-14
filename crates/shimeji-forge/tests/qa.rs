//! QA gates. Synthetic frames, no network.

use std::time::Duration;

use shimeji_forge::provider::{
    AgentMode, Background, Gemini, GenerateRequest, ImageProvider, OpenAi,
};
use shimeji_forge::{
    BBox, Edge, FrameTemplate, GROUPS, QaFrame, QaInput, RgbaImage, TemplateSet, decide_export, qa,
    save_png,
};

fn input(frames: Vec<QaFrame>, expected: usize) -> QaInput {
    QaInput {
        key: [0, 255, 0],
        expected_slots: expected,
        frames,
        templates: None,
    }
}

#[test]
fn an_empty_slot_fails_the_slot_count() {
    let frames = vec![
        QaFrame::lone(0, None, RgbaImage::solid(30, 80, [180, 40, 60, 255])),
        QaFrame::lone(1, None, RgbaImage::empty()),
        QaFrame::lone(2, None, RgbaImage::solid(32, 80, [40, 40, 180, 255])),
    ];
    let report = qa(&input(frames, 3));
    assert!(!report.slot_count_pass);
    assert_eq!(report.empty_slots, vec![1]);
    assert!(!report.passed);
    assert!(report.failed.iter().any(|name| name == "slot-count"));
}

#[test]
fn a_frame_fifteen_percent_off_the_family_median_fails_size() {
    let frames = vec![
        QaFrame::lone(0, Some(1), RgbaImage::solid(40, 100, [180, 40, 60, 255])),
        QaFrame::lone(1, Some(2), RgbaImage::solid(41, 100, [180, 50, 60, 255])),
        QaFrame::lone(2, Some(3), RgbaImage::solid(42, 115, [180, 40, 70, 255])),
    ];
    let report = qa(&input(frames, 3));
    assert!(report.slot_count_pass);
    assert!(report.duplicate_pass);
    assert!(!report.size_pass, "15% off the walk median must fail");
    assert!(
        report
            .size_outliers
            .iter()
            .any(|item| item.index == Some(3))
    );
    assert!(report.failed.iter().any(|name| name == "size"));
}

#[test]
fn identical_walk_frames_fail_the_duplicate_check() {
    let sprite = RgbaImage::solid(40, 100, [180, 40, 60, 255]);
    let frames = vec![
        QaFrame::lone(0, Some(2), sprite.clone()),
        QaFrame::lone(1, Some(3), sprite),
    ];
    let report = qa(&input(frames, 2));
    assert!(report.slot_count_pass);
    assert!(report.size_pass);
    assert!(!report.duplicate_pass);
    assert!(
        report
            .duplicate_pairs
            .iter()
            .any(|pair| pair.a == 2 && pair.b == 3)
    );
}

#[test]
fn export_is_blocked_unless_the_override_is_recorded() {
    let frames = vec![
        QaFrame::lone(0, None, RgbaImage::solid(30, 80, [180, 40, 60, 255])),
        QaFrame::lone(1, None, RgbaImage::empty()),
    ];
    let report = qa(&input(frames, 2));
    assert!(!report.passed);

    let blocked = decide_export(&report, false);
    assert!(!blocked.allowed);
    assert!(blocked.override_recorded.is_none());

    let allowed = decide_export(&report, true);
    assert!(allowed.allowed);
    let record = allowed.override_recorded.expect("override is recorded");
    assert!(record.failed.iter().any(|name| name == "slot-count"));
}

#[test]
fn a_padded_canvas_baseline_uses_the_cell_contact() {
    let templates = TemplateSet {
        frames: vec![FrameTemplate {
            index: 1,
            name: "stand".to_string(),
            bbox: BBox {
                x: 20,
                y: 1,
                w: 91,
                h: 127,
            },
            edge: Edge::Bottom,
            contact_at: 128,
            anchor_x: 64,
            anchor_y: 128,
            head_to_eye: 33,
        }],
    };
    // Feet sit on y=200 because the canvas grew upward by 72. In cell space that is y=128.
    let mut frame = QaFrame::lone(0, Some(1), RgbaImage::solid(40, 200, [180, 40, 60, 255]));
    frame.pad_y = 72;
    let mut spec = input(vec![frame], 1);
    spec.templates = Some(templates);
    let report = qa(&spec);
    assert!(report.baseline_pass, "{:?}", report.baseline_errors);
    assert!(report.failed.iter().all(|name| name != "baseline"));
}

#[test]
fn paid_providers_are_not_configured() {
    let request = GenerateRequest {
        prompt: "a mascot".to_string(),
        width: 128,
        height: 128,
        background: Background::Key([0, 255, 0]),
        n: 1,
    };
    let openai = OpenAi.generate(&request).unwrap_err();
    let gemini = Gemini.generate(&request).unwrap_err();
    assert!(openai.to_string().contains("not configured"));
    assert!(gemini.to_string().contains("not configured"));
}

#[test]
fn agent_mode_writes_jobs_and_waits_for_files() {
    let dir = std::env::temp_dir().join(format!(
        "shimeji-forge-agent-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mode = AgentMode {
        dir: dir.clone(),
        timeout: Duration::from_secs(0),
    };
    let path = mode.write_jobs("specimen").expect("jobs");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("socket"));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["mode"], "agent");
    assert_eq!(value["jobs"].as_array().unwrap().len(), GROUPS.len());
    for group in GROUPS {
        let job = value["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["id"] == group.id)
            .unwrap();
        assert_eq!(job["output"], format!("{}.png", group.id));
        assert_eq!(job["frames"].as_array().unwrap().len(), group.frames.len());
    }
    let missing = mode.wait_for_outputs().unwrap_err();
    assert!(missing.contains("have not appeared"));

    for group in GROUPS {
        save_png(
            &dir.join(format!("{}.png", group.id)),
            &RgbaImage::solid(2, 2, [180, 40, 60, 255]),
        )
        .unwrap();
    }
    mode.wait_for_outputs().expect("files appeared");
    let _ = std::fs::remove_dir_all(&dir);
}
