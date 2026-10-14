//! Right-click menu. The item list is pure; AppKit builds the `NSMenu`.
//!
//! R31 items the engine can do today: dismiss this pet, dismiss all of this
//! character, follow cursor (ChaseMouse), pause/resume, and the Toggleable
//! behaviors. "Restore moved windows" is Unit 11 and stays disabled.

/// What a click asks the shell to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    DismissOne,
    DismissAll,
    FollowCursor,
    TogglePause,
    ToggleBehavior(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub title: String,
    pub action: Option<MenuAction>,
    pub enabled: bool,
    pub checked: bool,
    pub children: Vec<MenuEntry>,
}

/// Inputs the shell already knows. Toggle rows may be empty (shime's are).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    pub user_paused: bool,
    /// `(name, currently enabled)`.
    pub toggles: Vec<(String, bool)>,
}

pub fn entries(model: &MenuModel) -> Vec<MenuEntry> {
    let toggles: Vec<MenuEntry> = model
        .toggles
        .iter()
        .enumerate()
        .map(|(index, (name, enabled))| MenuEntry {
            title: name.clone(),
            action: Some(MenuAction::ToggleBehavior(index)),
            enabled: true,
            checked: *enabled,
            children: Vec::new(),
        })
        .collect();
    let behaviors_enabled = !toggles.is_empty();
    vec![
        item("Dismiss", MenuAction::DismissOne),
        item("Dismiss All", MenuAction::DismissAll),
        item("Follow Cursor", MenuAction::FollowCursor),
        MenuEntry {
            title: "Restore Moved Windows".to_string(),
            action: None,
            enabled: false,
            checked: false,
            children: Vec::new(),
        },
        MenuEntry {
            title: "Behaviors".to_string(),
            action: None,
            enabled: behaviors_enabled,
            checked: false,
            children: toggles,
        },
        MenuEntry {
            title: if model.user_paused {
                "Resume".to_string()
            } else {
                "Pause".to_string()
            },
            action: Some(MenuAction::TogglePause),
            enabled: true,
            checked: false,
            children: Vec::new(),
        },
    ]
}

fn item(title: &str, action: MenuAction) -> MenuEntry {
    MenuEntry {
        title: title.to_string(),
        action: Some(action),
        enabled: true,
        checked: false,
        children: Vec::new(),
    }
}

pub const TAG_DISMISS: isize = 1;
pub const TAG_DISMISS_ALL: isize = 2;
pub const TAG_FOLLOW: isize = 3;
pub const TAG_PAUSE: isize = 4;
pub const TAG_TOGGLE: isize = 1000;

#[cfg(target_os = "macos")]
pub fn popup(mtm: objc2::MainThreadMarker, model: &MenuModel, target: &objc2::runtime::AnyObject) {
    use objc2::MainThreadOnly;
    use objc2_app_kit::NSMenu;
    use objc2_foundation::NSString;

    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Shimeji"));
    menu.setAutoenablesItems(false);
    for entry in entries(model) {
        add_entry(mtm, &menu, &entry, target);
    }
    // Blocks in `NSEventTrackingRunLoopMode` for as long as the menu is open.
    // The GCD clock is on the main queue, which is scheduled in the common
    // modes, and tracking mode is one of them, so ticks keep running. The
    // reference's ticker is a background thread and keeps going too; it only
    // freezes the clicked mascot (`setAnimating(false)`). This engine has no
    // per-mascot animating flag, so the clicked pet keeps ticking as well.
    let _hit = menu.popUpMenuPositioningItem_atLocation_inView(
        None,
        objc2_app_kit::NSEvent::mouseLocation(),
        None,
    );
}

#[cfg(target_os = "macos")]
fn add_entry(
    mtm: objc2::MainThreadMarker,
    menu: &objc2_app_kit::NSMenu,
    entry: &MenuEntry,
    target: &objc2::runtime::AnyObject,
) {
    use objc2::MainThreadOnly;
    use objc2::sel;
    use objc2_app_kit::{NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem};
    use objc2_foundation::NSString;

    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(&entry.title),
            entry.action.as_ref().map(|_| sel!(menuAct:)),
            &NSString::from_str(""),
        )
    };
    item.setEnabled(entry.enabled);
    item.setState(if entry.checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    if let Some(action) = &entry.action {
        let tag = match action {
            MenuAction::DismissOne => TAG_DISMISS,
            MenuAction::DismissAll => TAG_DISMISS_ALL,
            MenuAction::FollowCursor => TAG_FOLLOW,
            MenuAction::TogglePause => TAG_PAUSE,
            MenuAction::ToggleBehavior(index) => TAG_TOGGLE + *index as isize,
        };
        item.setTag(tag);
        unsafe { item.setTarget(Some(target)) };
    }
    if !entry.children.is_empty() || entry.title == "Behaviors" {
        let sub = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&entry.title));
        sub.setAutoenablesItems(false);
        for child in &entry.children {
            add_entry(mtm, &sub, child, target);
        }
        item.setSubmenu(Some(&sub));
        item.setEnabled(entry.enabled);
    }
    menu.addItem(&item);
}
