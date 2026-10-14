//! Menu-bar item. The row list is data; AppKit builds it on `menuNeedsUpdate`.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem, NSStatusBar, NSWorkspace,
};
use objc2_foundation::{NSString, NSURL};

use crate::mischief::menu_item;

use crate::bridge::ShellClock;

/// Tags on the status menu. The context menu uses 1–4 and 1000+, so these stay apart.
pub const TAG_PAUSE: isize = 10;
pub const TAG_HIDE: isize = 11;
pub const TAG_DISMISS_ALL: isize = 12;
pub const TAG_MISCHIEF: isize = 13;
pub const TAG_SETTINGS: isize = 14;
pub const TAG_QUIT: isize = 15;
pub const TAG_UPDATES: isize = 16;
pub const TAG_ADD: isize = 20_000;
pub const TAG_REMOVE: isize = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterCount {
    pub pack: String,
    pub name: String,
    pub live: usize,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusModel {
    pub paused: bool,
    pub hidden: bool,
    pub at_cap: bool,
    pub characters: Vec<CharacterCount>,
}

pub fn character_label(row: &CharacterCount) -> String {
    format!("{} ({})", row.name, row.live)
}

pub fn pause_title(paused: bool) -> &'static str {
    if paused { "Resume" } else { "Pause" }
}

pub fn hide_title(hidden: bool) -> &'static str {
    if hidden { "Show All" } else { "Hide All" }
}

pub fn install(clock: &ShellClock) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
    if let Some(button) = item.button(mtm) {
        button.setTitle(&NSString::from_str("し"));
    }
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Shimeji"));
    menu.setAutoenablesItems(false);
    let delegate = ProtocolObject::from_ref(clock);
    menu.setDelegate(Some(delegate));
    item.setMenu(Some(&menu));
    println!("status_item=1");
    *clock.ivars().status_item.borrow_mut() = Some(item);
    *clock.ivars().status_menu.borrow_mut() = Some(menu);
}

pub fn rebuild(clock: &ShellClock, menu: &NSMenu) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let look = menu_item(clock.ivars().state.borrow().mischief.phase());
    let model = super::actions::status_model(clock);
    menu.removeAllItems();
    let target = std::ptr::from_ref(clock).cast::<AnyObject>();
    let target = unsafe { &*target };

    let characters = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Characters"));
    characters.setAutoenablesItems(false);
    if model.characters.is_empty() {
        let empty = row(mtm, "No characters", None, false, target);
        characters.addItem(&empty);
    }
    for (index, character) in model.characters.iter().enumerate() {
        let parent = row(mtm, &character_label(character), None, true, target);
        let sub = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Character"));
        sub.setAutoenablesItems(false);
        let add = row(
            mtm,
            "Add",
            Some(TAG_ADD + index as isize),
            character.enabled && !model.at_cap,
            target,
        );
        let remove = row(
            mtm,
            "Remove Newest",
            Some(TAG_REMOVE + index as isize),
            character.live > 0,
            target,
        );
        sub.addItem(&add);
        sub.addItem(&remove);
        parent.setSubmenu(Some(&sub));
        characters.addItem(&parent);
    }
    let characters_item = row(mtm, "Characters", None, true, target);
    characters_item.setSubmenu(Some(&characters));
    menu.addItem(&characters_item);
    menu.addItem(&row(
        mtm,
        pause_title(model.paused),
        Some(TAG_PAUSE),
        true,
        target,
    ));
    menu.addItem(&row(
        mtm,
        hide_title(model.hidden),
        Some(TAG_HIDE),
        true,
        target,
    ));
    menu.addItem(&row(
        mtm,
        "Dismiss All",
        Some(TAG_DISMISS_ALL),
        true,
        target,
    ));
    let mischief = row(mtm, look.title, Some(TAG_MISCHIEF), true, target);
    mischief.setState(if look.checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    menu.addItem(&mischief);
    menu.addItem(&row(mtm, "Settings…", Some(TAG_SETTINGS), true, target));
    menu.addItem(&row(
        mtm,
        "Check for Updates…",
        Some(TAG_UPDATES),
        crate::updates::releases_url().is_some(),
        target,
    ));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&row(mtm, "Quit", Some(TAG_QUIT), true, target));
}

/// Hand the releases page to the system. This process does not request it.
pub fn open_releases() {
    let Some(releases) = crate::updates::releases_url() else {
        return;
    };
    let text = NSString::from_str(releases);
    let Some(url) = NSURL::URLWithString(&text) else {
        eprintln!("updates_open=0");
        return;
    };
    let opened = NSWorkspace::sharedWorkspace().openURL(&url);
    println!("updates_open={}", u8::from(opened));
}

fn row(
    mtm: MainThreadMarker,
    title: &str,
    tag: Option<isize>,
    enabled: bool,
    target: &AnyObject,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            tag.map(|_| sel!(statusAct:)),
            &NSString::from_str(""),
        )
    };
    item.setEnabled(enabled);
    if let Some(tag) = tag {
        item.setTag(tag);
        unsafe { item.setTarget(Some(target)) };
    }
    item
}
