//! Context-menu rows. Popping the NSMenu is manual.

use shimeji_macos::platform::context_menu::{MenuAction, MenuModel, entries};

#[test]
fn shime_menu_disables_restore_and_an_empty_behavior_list() {
    let model = MenuModel {
        user_paused: false,
        toggles: Vec::new(),
    };
    let rows = entries(&model);
    let titles: Vec<_> = rows.iter().map(|row| row.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Dismiss",
            "Dismiss All",
            "Follow Cursor",
            "Restore Moved Windows",
            "Behaviors",
            "Pause",
        ]
    );
    let restore = rows
        .iter()
        .find(|row| row.title == "Restore Moved Windows")
        .unwrap();
    assert!(!restore.enabled);
    assert!(restore.action.is_none());
    let behaviors = rows.iter().find(|row| row.title == "Behaviors").unwrap();
    assert!(!behaviors.enabled);
    assert!(behaviors.children.is_empty());
    assert_eq!(rows[0].action, Some(MenuAction::DismissOne));
    assert_eq!(rows[2].action, Some(MenuAction::FollowCursor));
}

#[test]
fn pause_label_flips_and_toggles_are_checked() {
    let model = MenuModel {
        user_paused: true,
        toggles: vec![("Spin".to_string(), true), ("Sit".to_string(), false)],
    };
    let rows = entries(&model);
    assert_eq!(rows.last().unwrap().title, "Resume");
    assert_eq!(rows.last().unwrap().action, Some(MenuAction::TogglePause));
    let behaviors = rows.iter().find(|row| row.title == "Behaviors").unwrap();
    assert!(behaviors.enabled);
    assert_eq!(behaviors.children.len(), 2);
    assert!(behaviors.children[0].checked);
    assert!(!behaviors.children[1].checked);
    assert_eq!(
        behaviors.children[1].action,
        Some(MenuAction::ToggleBehavior(1))
    );
}
