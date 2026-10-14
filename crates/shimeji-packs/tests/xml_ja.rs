//! Japanese documents: the bundled shime and the classic fixture.

use std::path::PathBuf;

use shimeji_core::model::{ActionKind, Builtin, LayoutKind};
use shimeji_packs::load_dir;

fn shime_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime")
}

#[test]
fn bundled_shime_loads_with_no_diagnostics() {
    let outcome = load_dir(shime_root()).unwrap();
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    assert_eq!(outcome.pack.layout, LayoutKind::ClassicSingle);
    assert_eq!(outcome.pack.characters.len(), 1);
    let character = &outcome.pack.characters[0];
    assert_eq!(character.name, "shime");
    assert_eq!(character.image_root, "img");
    assert_eq!(
        character
            .constants
            .get("maxCount")
            .map(|c| c.value.as_str()),
        Some("50")
    );
    let mut kinds = Vec::new();
    let mut builtins = Vec::new();
    character.visit_actions(&mut |action| {
        kinds.push(action.kind);
        if let ActionKind::Builtin(builtin) = action.kind {
            builtins.push(builtin);
        }
    });
    for kind in [
        ActionKind::Move,
        ActionKind::Stay,
        ActionKind::Animate,
        ActionKind::Sequence,
        ActionKind::Select,
    ] {
        assert!(kinds.contains(&kind), "missing {kind:?}");
    }
    for builtin in [
        Builtin::Fall,
        Builtin::Jump,
        Builtin::Dragged,
        Builtin::Regist,
        Builtin::Breed,
        Builtin::Look,
        Builtin::Offset,
        Builtin::WalkWithIe,
        Builtin::FallWithIe,
        Builtin::ThrowIe,
    ] {
        assert!(builtins.contains(&builtin), "missing {builtin:?}");
    }
    let breed = character.actions.get("引っこ抜く1").unwrap();
    let born_x = breed.params.get("BornX").unwrap();
    assert_eq!(born_x.source_name, "生まれる場所X");
    assert_eq!(born_x.value.raw, "-32");
    assert!(
        character
            .behavior_tree
            .iter()
            .any(|node| { matches!(node, shimeji_core::model::BehaviorNode::Group { .. }) })
    );
    assert!(
        character
            .behaviors
            .values()
            .any(|behavior| !behavior.next_lists.is_empty())
    );
    let stand = character.actions.get("立つ").unwrap();
    let image = stand.animations[0].poses[0].image.as_ref().unwrap();
    assert_eq!(image.relative, "shime1.png");
    assert_eq!(image.source, "/shime1.png");
}

#[path = "common/mod.rs"]
mod common;

#[test]
fn minimal_classic_keeps_reference_quirks() {
    let outcome = load_dir(common::fixture("minimal-classic")).unwrap();
    assert!(outcome.diagnostics.is_empty(), "{:#?}", outcome.diagnostics);
    assert_eq!(outcome.pack.layout, LayoutKind::ImageSets);
    assert!(outcome.pack.character("unused").is_none());
    let character = outcome.pack.character("classic").unwrap();
    assert_eq!(character.image_root, "img/classic");

    let keys: Vec<_> = character.constants.keys().collect();
    assert_eq!(keys, ["maxCount", "later"]);
    assert_eq!(character.constants.get("maxCount").unwrap().value, "50");

    let names: Vec<_> = character.actions.keys().collect();
    let redefined = names.iter().position(|name| *name == "再定義").unwrap();
    let later = names.iter().position(|name| *name == "先").unwrap();
    assert!(redefined < later);
    assert!(matches!(
        character.actions.get("再定義").unwrap().kind,
        ActionKind::Move
    ));
    assert!(matches!(
        character.actions.get("ずらす").unwrap().kind,
        ActionKind::Builtin(Builtin::Offset)
    ));
    assert!(matches!(
        character.actions.get("落下アクション").unwrap().kind,
        ActionKind::Builtin(Builtin::Fall)
    ));
    let breed = character.actions.get("増える").unwrap();
    assert_eq!(
        breed.params.get("BornX").unwrap().source_name,
        "生まれる場所X"
    );
    assert_eq!(breed.params.get("BornX").unwrap().value.raw, "-32");
    assert_eq!(breed.params.get("BornY").unwrap().value.raw, "96");
    assert_eq!(
        breed.params.get("BornBehavior").unwrap().source_name,
        "生まれた時の行動"
    );

    let sequence = character.actions.get("落ちる").unwrap();
    let shimeji_core::model::ActionNode::Action(select) = &sequence.children[0] else {
        panic!("sequence child");
    };
    assert!(matches!(select.kind, ActionKind::Select));
    assert!(select.name.is_none());

    let pose = &character.actions.get("歩く").unwrap().animations[0].poses[0];
    assert_eq!(pose.velocity_x, -2);
    assert_eq!(pose.duration_ticks, 6);
    assert_eq!(pose.image.as_ref().unwrap().relative, "shime.png");

    let chase = character.behaviors.get("マウスの周りに集まる").unwrap();
    assert_eq!(chase.params.get("メモ").unwrap().value.raw, "from-target");
    assert!(!chase.hidden);
    let falling = character.behaviors.get("落下する").unwrap();
    assert!(!falling.toggleable);
    assert!(character.behaviors.get("投げられる").unwrap().hidden);

    let behavior = character.behaviors.get("立つ行動").unwrap();
    assert!(behavior.toggleable);
    assert_eq!(behavior.conditions.len(), 3);
    assert_eq!(
        behavior.conditions[0].wrapper,
        Some(shimeji_core::model::ScriptKind::EveryTick)
    );
    assert!(behavior.conditions[0].ast.is_some());
    assert_eq!(behavior.conditions[1].raw, "${1}");
    assert!(behavior.conditions[1].ast.is_some());
    assert_eq!(behavior.conditions[2].raw, "true");
    assert!(behavior.conditions[2].wrapper.is_none());
    assert!(!behavior.next_additive);
    assert_eq!(behavior.next_lists.len(), 2);
    assert!(behavior.next_lists[0].add);
    assert!(!behavior.next_lists[1].add);
    let reference = &behavior.next_lists[0].refs[0];
    assert_eq!(reference.frequency, 1);
    assert!(reference.params.get("メモ").is_none());
    assert_eq!(reference.params.get("BornX").unwrap().value.raw, "9");
    assert!(reference.conditions.is_empty());
    let nested = &behavior.next_lists[1].refs[0];
    assert_eq!(nested.conditions.len(), 2);
    assert_eq!(nested.conditions[0].raw, "${1}");
    assert_eq!(nested.conditions[1].raw, "false");

    let group = character
        .behavior_tree
        .iter()
        .find_map(|node| match node {
            shimeji_core::model::BehaviorNode::Group {
                condition,
                children,
                ..
            } => Some((condition, children)),
            _ => None,
        })
        .unwrap();
    assert_eq!(group.0.as_ref().unwrap().raw, "#{mascot.lookRight}");
    assert!(matches!(
        group.1.first(),
        Some(shimeji_core::model::BehaviorNode::Group { .. })
    ));
}
