//! English documents, including the two-character override pack.

#[path = "common/mod.rs"]
mod common;

use shimeji_core::model::{ActionKind, FastFollow, LayoutKind};
use shimeji_packs::load_dir;

#[test]
fn minimal_ee_loads_two_characters_and_the_override() {
    let outcome = load_dir(common::fixture("minimal-ee")).unwrap();
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    assert_eq!(outcome.pack.layout, LayoutKind::ImageSets);
    assert_eq!(outcome.pack.characters.len(), 2);

    let a = outcome.pack.character("A").unwrap();
    assert!(a.actions.contains("Shared"));
    assert!(!a.actions.contains("OnlyB"));
    let sit = a.actions.get("Sit").unwrap();
    assert_eq!(
        sit.params.get("ResistanceX").unwrap().source_name,
        "RegistanceX"
    );
    assert_eq!(sit.params.get("ResistanceX").unwrap().value.raw, "0.1");
    assert_eq!(
        sit.params.get("ResistanceY").unwrap().source_name,
        "ResistanceY"
    );
    let pose = &sit.animations[0].poses[0];
    assert_eq!(pose.image.as_ref().unwrap().relative, "a.png");
    assert_eq!(
        (
            pose.image.as_ref().unwrap().width,
            pose.image.as_ref().unwrap().height
        ),
        (1, 1)
    );
    assert!(pose.image_right.is_none());
    assert_eq!(pose.right_sibling.as_ref().unwrap().relative, "a-r.png");
    assert_eq!(pose.right_sibling.as_ref().unwrap().width, 1);
    assert!(a.behaviors.get("Thrown").unwrap().hidden);

    let b = outcome.pack.character("B").unwrap();
    assert!(b.actions.contains("OnlyB"));
    assert!(!b.actions.contains("Shared"));
    assert!(matches!(
        b.actions.get("OnlyB").unwrap().kind,
        ActionKind::Move
    ));
    let pose = &b.actions.get("Sit").unwrap().animations[0].poses[0];
    assert_eq!(pose.image.as_ref().unwrap().relative, "b.png");
    assert_eq!(pose.image.as_ref().unwrap().height, 1);
    assert_eq!(pose.image_right.as_ref().unwrap().relative, "b-right.png");
    assert_eq!(pose.image_right.as_ref().unwrap().source, "/b-right.png");
    assert_eq!(
        (
            pose.image_right.as_ref().unwrap().width,
            pose.image_right.as_ref().unwrap().height
        ),
        (1, 1)
    );
    assert!(pose.right_sibling.is_none());
    assert!(b.behaviors.contains("ChaseMouse"));
}

#[test]
fn fast_follow_classes_stay_in_the_model() {
    let dir = std::env::temp_dir().join(format!("shimeji-u5a-follow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    common::write_minimal_en(
        &dir,
        r#"
    <Action Name="Sit" Type="Stay">
      <Animation>
        <Pose Image="/p.png" ImageAnchor="0,0" Velocity="0,0" Duration="1"/>
      </Animation>
    </Action>
    <Action Name="Scan" Type="Embedded" Class="ScanMove"/>
    <Action Name="ScanFull" Type="Embedded" Class="com.group_finity.mascot.action.Transform"/>
"#,
    );
    let outcome = load_dir(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        outcome
            .diagnostics
            .iter()
            .all(|diag| !diag.is_stay_fallback())
    );
    assert_eq!(
        outcome
            .diagnostics
            .iter()
            .filter(|diag| diag.kind == shimeji_packs::DiagnosticKind::FastFollowClass)
            .count(),
        2
    );
    assert!(outcome.diagnostics.iter().all(|diag| {
        diag.kind != shimeji_packs::DiagnosticKind::FastFollowClass
            || (diag.severity == shimeji_packs::Severity::Info && diag.fallback.is_none())
    }));
    let character = &outcome.pack.characters[0];
    assert!(matches!(
        character.actions.get("Scan").unwrap().kind,
        ActionKind::FastFollow(FastFollow::ScanMove)
    ));
    assert!(matches!(
        character.actions.get("ScanFull").unwrap().kind,
        ActionKind::FastFollow(FastFollow::Transform)
    ));
}
