//! Drag a folder or a zip onto the Pets tab.
//!
//! The outcome text is pure. The duplicate question is an `NSAlert`.

use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker};
#[allow(deprecated)]
use objc2_app_kit::NSFilenamesPboardType;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSPasteboard,
    NSPasteboardTypeFileURL,
};
use objc2_foundation::{NSString, NSURL};
use shimeji_app::import::{self, Choice, Outcome};

use crate::bridge::ShellClock;

pub fn banner(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Success(installed) => format!(
            "Imported {} ({} characters).",
            installed.id, installed.characters
        ),
        Outcome::Degraded(installed) => {
            let detail = installed
                .warnings
                .first()
                .map(|warning| format!("{}: {}", warning_place(warning), warning.message))
                .unwrap_or_else(|| "a feature was degraded".to_string());
            format!(
                "Imported {} ({} characters) with a warning. {detail}",
                installed.id, installed.characters
            )
        }
        Outcome::Error { message } => message.clone(),
        Outcome::Duplicate { id } => format!("{id} is already installed."),
    }
}

fn warning_place(warning: &shimeji_app::import::Warning) -> String {
    match warning.line {
        Some(line) => format!("{}:{line}", warning.file),
        None => warning.file.clone(),
    }
}

/// Replace, keep both, or leave the installed pack alone.
pub fn ask_duplicate(mtm: MainThreadMarker, id: &str) -> Option<Choice> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&format!("{id} is already installed")));
    alert.setInformativeText(&NSString::from_str(
        "Replace swaps the installed files. Keep Both adds a suffixed copy.",
    ));
    alert.addButtonWithTitle(&NSString::from_str("Replace"));
    alert.addButtonWithTitle(&NSString::from_str("Keep Both"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    let response = alert.runModal();
    if response == NSAlertFirstButtonReturn {
        Some(Choice::Replace)
    } else if response == NSAlertSecondButtonReturn {
        Some(Choice::KeepBoth)
    } else {
        None
    }
}

/// Install is the first button. Anything else, including a missing main
/// thread, is Ignore: forge never imports without this dialog.
pub fn ask_forge_install(mtm: MainThreadMarker, prompt: &str) -> bool {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(prompt));
    alert.addButtonWithTitle(&NSString::from_str("Install"));
    alert.addButtonWithTitle(&NSString::from_str("Ignore"));
    alert.runModal() == NSAlertFirstButtonReturn
}

pub fn dropped_paths(pasteboard: &NSPasteboard) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[allow(deprecated)]
    let filenames = unsafe { NSFilenamesPboardType };
    if let Some(list) = pasteboard.propertyListForType(filenames) {
        let count: usize = unsafe { objc2::msg_send![&*list, count] };
        for index in 0..count {
            let item: Retained<AnyObject> =
                unsafe { objc2::msg_send![&*list, objectAtIndex: index] };
            let Ok(text) = item.downcast::<NSString>() else {
                continue;
            };
            let path = super::lossy(&text);
            if !path.is_empty() {
                paths.push(PathBuf::from(path));
            }
        }
    }
    if paths.is_empty() {
        if let Some(items) = pasteboard.pasteboardItems() {
            for item in items.iter() {
                let Some(text) = item.stringForType(unsafe { NSPasteboardTypeFileURL }) else {
                    continue;
                };
                if let Some(url) = NSURL::URLWithString(&text) {
                    if let Some(path) = url.path() {
                        let path = super::lossy(&path);
                        if !path.is_empty() {
                            paths.push(PathBuf::from(path));
                        }
                    }
                }
            }
        }
    }
    paths
}

/// Import each dropped path. A duplicate asks replace or keep-both before a second call.
pub fn accept_drop(clock: &ShellClock, packs_root: &Path, paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "Drop a pack folder or a zip.".to_string();
    }
    let mut last = String::new();
    for path in paths {
        let outcome = import::import(path, packs_root, None);
        let outcome = if let Outcome::Duplicate { id } = &outcome {
            let Some(mtm) = MainThreadMarker::new() else {
                return "not on the main thread".to_string();
            };
            match ask_duplicate(mtm, id) {
                Some(choice) => import::import(path, packs_root, Some(choice)),
                None => {
                    last = format!("Kept the installed {id}.");
                    continue;
                }
            }
        } else {
            outcome
        };
        last = banner(&outcome);
        println!("import_banner={last}");
        super::actions::note_import(clock, outcome);
    }
    last
}

/// User-initiated import shares the exact drop/duplicate handling path.
pub fn choose(clock: &ShellClock) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let panel = objc2_app_kit::NSOpenPanel::openPanel(mtm);
    panel.setTitle(Some(&NSString::from_str("导入宠物素材")));
    panel.setPrompt(Some(&NSString::from_str("导入")));
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    if panel.runModal() != objc2_app_kit::NSModalResponseOK {
        return;
    }
    let Some(url) = panel.URL() else {
        return;
    };
    let Some(path) = url.path() else {
        return;
    };
    let directory = clock.ivars().state.borrow().support_dir.clone();
    let message = accept_drop(clock, &directory, &[PathBuf::from(super::lossy(&path))]);
    super::actions::set_import_banner(clock, &message);
}
