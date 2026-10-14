//! Settings window. Pets, Behavior, Mischief, Advanced, and Agents.

mod advanced_tab;
pub(crate) mod agents_tab;
mod behavior_tab;
pub(crate) mod diagnostics_sheet;
mod form;
pub(crate) mod mischief_tab;
mod pets_tab;

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions as Resize, NSBackingStoreType, NSBox, NSButton, NSButtonType,
    NSCellImagePosition, NSColor, NSControl, NSControlStateValueOff, NSControlStateValueOn, NSFont,
    NSImage, NSPopUpButton, NSScrollView, NSSwitch, NSTextAlignment, NSTextField, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSSize, NSString};
use shimeji_app::mischief_state::Phase;
use shimeji_app::window_rules::WindowRules;

use crate::bridge::ShellClock;

use super::rect;

pub use pets_tab::OutlineSource;

pub struct SettingsWindow {
    pub window: Retained<NSWindow>,
    pages: Vec<Retained<NSScrollView>>,
    navigation: Vec<Retained<NSButton>>,
    selection: Retained<NSBox>,
    values: Vec<(isize, Retained<NSTextField>)>,
    outline: Retained<pets_tab::PetsOutline>,
    source: Retained<OutlineSource>,
    banner: Retained<NSTextField>,
    controls: Vec<Retained<NSControl>>,
    mischief_enable: Retained<NSSwitch>,
    mischief_phase: Retained<NSTextField>,
    mischief_defaults: Vec<(String, Retained<NSButton>)>,
    mischief_rules: Retained<NSPopUpButton>,
    mischief_remove: Retained<NSButton>,
    mischief_pattern: Retained<NSTextField>,
    mischief_target: Retained<NSPopUpButton>,
    mischief_effect: Retained<NSPopUpButton>,
    pub agents: agents_tab::AgentsTab,
}

impl SettingsWindow {
    pub fn open(clock: &ShellClock) -> Self {
        let Some(mtm) = MainThreadMarker::new() else {
            panic!("settings window off the main thread");
        };
        let pets = pets_tab::build(mtm, clock);
        let behavior = behavior_tab::build(mtm, clock);
        let mischief = mischief_tab::build(mtm, clock);
        let advanced = advanced_tab::build(mtm, clock);
        let agents = agents_tab::build(mtm, clock);
        let mut controls = behavior.controls;
        controls.extend(advanced.1);

        let root = form::view(mtm, 840.0, 660.0);
        root.setAutoresizingMask(Resize::ViewWidthSizable | Resize::ViewHeightSizable);
        let sidebar = form::view(mtm, 180.0, 660.0);
        sidebar.setAutoresizingMask(Resize::ViewHeightSizable);
        let background = form::box_view(
            &root,
            rect(0.0, 0.0, 180.0, 660.0),
            &NSColor::underPageBackgroundColor(),
            mtm,
        );
        background.setAutoresizingMask(Resize::ViewHeightSizable);
        root.addSubview(&sidebar);
        form::text(
            &sidebar,
            "设置",
            rect(20.0, 28.0, 140.0, 30.0),
            23.0,
            false,
            true,
            mtm,
        );
        form::text(
            &sidebar,
            "桌面宠物",
            rect(20.0, 64.0, 140.0, 22.0),
            11.0,
            true,
            false,
            mtm,
        );
        let selection = form::box_view(
            &sidebar,
            rect(12.0, 116.0, 156.0, 38.0),
            &NSColor::controlAccentColor(),
            mtm,
        );
        selection.setAutoresizingMask(Resize::empty());
        let target = unsafe { &*std::ptr::from_ref(clock).cast::<objc2::runtime::AnyObject>() };
        let mut pages = Vec::new();
        let mut navigation = Vec::new();
        for (index, (view, title, symbol)) in [
            (&pets.view, "宠物", "pawprint"),
            (&behavior.view, "外观与动作", "slider.horizontal.3"),
            (&mischief.view, "窗口互动", "macwindow"),
            (&agents.view, "智能体", "sparkles"),
            (&advanced.0, "高级", "gearshape"),
        ]
        .into_iter()
        .enumerate()
        {
            let button = form::button(
                &sidebar,
                title,
                rect(18.0, 118.0 + index as f64 * 46.0, 144.0, 34.0),
                index as isize,
                target,
                objc2::sel!(settingsPage:),
                mtm,
            );
            button.setBordered(false);
            button.setButtonType(NSButtonType::PushOnPushOff);
            button.setAlignment(NSTextAlignment::Left);
            button.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, 0.0)));
            if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(symbol),
                None,
            ) {
                image.setSize(NSSize::new(18.0, 18.0));
                button.setImage(Some(&image));
                button.setImagePosition(NSCellImagePosition::ImageLeft);
            }
            navigation.push(button);
            let scroll = NSScrollView::initWithFrame(
                NSScrollView::alloc(mtm),
                rect(180.0, 0.0, 660.0, 660.0),
            );
            scroll.setHasVerticalScroller(true);
            scroll.setAutohidesScrollers(true);
            scroll.setDrawsBackground(false);
            scroll.setAutoresizingMask(Resize::ViewWidthSizable | Resize::ViewHeightSizable);
            scroll.setDocumentView(Some(view));
            root.addSubview(&scroll);
            pages.push(scroll);
        }
        let saved = form::text(
            &sidebar,
            "更改会自动保存",
            rect(20.0, 612.0, 140.0, 22.0),
            11.0,
            true,
            false,
            mtm,
        );
        saved.setAutoresizingMask(Resize::ViewMinYMargin);

        let style = NSWindowStyleMask(
            NSWindowStyleMask::Titled.0
                | NSWindowStyleMask::Closable.0
                | NSWindowStyleMask::Miniaturizable.0
                | NSWindowStyleMask::Resizable.0,
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, 840.0, 660.0),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str("设置"));
        window.setTitlebarAppearsTransparent(true);
        window.setContentMinSize(NSSize::new(840.0, 660.0));
        window.setContentView(Some(&root));
        window.center();
        unsafe { window.setReleasedWhenClosed(false) };

        let ui = Self {
            window,
            pages,
            navigation,
            selection,
            values: behavior.values,
            outline: pets.outline,
            source: pets.source,
            banner: pets.banner,
            controls,
            mischief_enable: mischief.enable,
            mischief_phase: mischief.phase,
            mischief_defaults: mischief.defaults,
            mischief_rules: mischief.rules,
            mischief_remove: mischief.remove,
            mischief_pattern: mischief.pattern,
            mischief_target: mischief.target,
            mischief_effect: mischief.effect,
            agents,
        };
        ui.select_page(0);
        ui.agents.sync(clock);
        ui.sync_from(clock);
        ui.reload_outline();
        ui
    }

    pub fn select_page(&self, index: usize) {
        if index >= self.pages.len() {
            return;
        }
        for (i, page) in self.pages.iter().enumerate() {
            page.setHidden(i != index);
        }
        self.selection
            .setFrameOrigin(NSPoint::new(12.0, 116.0 + index as f64 * 46.0));
        for (i, button) in self.navigation.iter().enumerate() {
            button.setState(if i == index {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            let tint = if i == index {
                NSColor::whiteColor()
            } else {
                NSColor::labelColor()
            };
            button.setContentTintColor(Some(&tint));
        }
    }

    pub fn sync_values(&self, scaling: f64, opacity: f64) {
        for (tag, label) in &self.values {
            let value = if *tag == 1 { scaling } else { opacity };
            label.setStringValue(&NSString::from_str(&format!("{:.0}%", value * 100.0)));
        }
    }

    pub fn set_banner(&self, text: &str) {
        let value = NSString::from_str(text);
        unsafe {
            let _: () = objc2::msg_send![&*self.banner, setStringValue: &*value];
        }
    }

    pub fn reload_outline(&self) {
        self.source.rebuild();
        unsafe {
            let _: () = objc2::msg_send![&*self.outline, reloadData];
        }
    }

    pub fn sync_from(&self, clock: &ShellClock) {
        let (settings, phase) = {
            let state = clock.ivars().state.borrow();
            (state.settings.clone(), state.mischief.phase())
        };
        self.sync_values(settings.scaling, settings.opacity);
        self.sync_mischief_status(phase, settings.mischief);
        self.sync_mischief_rules(&settings.window_rules);
        for control in &self.controls {
            match control.tag() {
                1 => control.setDoubleValue(settings.scaling),
                2 => {
                    let index: isize = match settings.filter {
                        shimeji_app::settings::ScaleFilter::Bicubic => 1,
                        shimeji_app::settings::ScaleFilter::Hqx => 2,
                        shimeji_app::settings::ScaleFilter::Nearest => 0,
                    };
                    unsafe {
                        let _: () = objc2::msg_send![&**control, selectItemAtIndex: index];
                    }
                }
                3 => control.setDoubleValue(settings.opacity),
                4 => set_check(control, settings.breeding),
                5 => set_check(control, settings.transients),
                6 => set_check(control, settings.transformation),
                7 => set_check(control, settings.throwing),
                8 => set_check(control, settings.sounds),
                9 => set_check(control, settings.multiscreen),
                10 => control.setStringValue(&NSString::from_str(&settings.mascot_cap.to_string())),
                11 => control.setStringValue(&NSString::from_str(&settings.hotkey)),
                12 => set_check(control, settings.animate_anyway),
                14 => set_check(control, settings.keep_on_screen),
                _ => {}
            }
        }
    }

    pub fn sync_mischief_status(&self, phase: Phase, enabled: bool) {
        set_check(&self.mischief_enable, enabled);
        let line = mischief_tab::status_line(phase);
        self.mischief_phase
            .setStringValue(&NSString::from_str(&line));
    }

    pub fn sync_mischief_rules(&self, rules: &WindowRules) {
        for (name, button) in &self.mischief_defaults {
            set_check(button, rules.default_allowed(name));
        }
        mischief_tab::fill_rules(&self.mischief_rules, &self.mischief_remove, rules);
    }

    pub fn rule_pattern(&self) -> String {
        super::lossy(&self.mischief_pattern.stringValue())
    }

    pub fn clear_rule_pattern(&self) {
        self.mischief_pattern
            .setStringValue(&NSString::from_str(""));
    }

    pub fn rule_target_index(&self) -> isize {
        self.mischief_target.indexOfSelectedItem()
    }

    pub fn rule_effect_index(&self) -> isize {
        self.mischief_effect.indexOfSelectedItem()
    }

    pub fn selected_rule_index(&self) -> isize {
        self.mischief_rules.indexOfSelectedItem()
    }

    pub fn sync_sounds(&self, on: bool) {
        for control in &self.controls {
            if control.tag() == 8 {
                set_check(control, on);
            }
        }
    }
}

fn set_check(control: &NSControl, on: bool) {
    let state = if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    };
    unsafe {
        let _: () = objc2::msg_send![control, setState: state];
    }
}
