//! Status-menu and settings actions. AppKit builds the controls; the decisions
//! go through the same `Power` and runtime commands as the hotkey and the
//! context menu.

use std::collections::VecDeque;
use std::sync::Arc;

use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSControlStateValue, NSControlStateValueOff, NSControlStateValueOn,
};
use objc2_foundation::NSString;
use shimeji_app::import::Outcome;
use shimeji_app::mischief_state::Phase;
use shimeji_app::settings::{self, ScaleFilter, Settings};
use shimeji_app::window_rules::{DEFAULT_BLOCKED_APPS, Effect, Target, WindowRules};
use shimeji_app::{Command, Runtime};
use shimeji_core::MascotId;
use shimeji_core::WindowControl;
use shimeji_core::env::{Point, Snapshot};
use shimeji_core::model::{Character, Pack};
use shimeji_packs::{DecodedImage, DecodedWav};

use crate::bridge::{AppState, FrozenDrag, ShellClock};
use crate::mischief::WindowHandle;
use crate::platform::context_menu;
use crate::platform::coords::CocoaSpace;
use crate::platform::image_cache::ImageCache;
use crate::platform::input::{InputEvent, InputQueue};
use crate::platform::panel::PanelRenderer;
use crate::platform::poses::PoseTable;
use crate::platform::render::Renderer;
use crate::platform::screens::read_space;
use crate::platform::sound::{self, Library as Clips};
use crate::platform::spawn::{spawn_anchor, take_fixups, take_ready, visible_drop_anchor};
use crate::platform::window_env::{PollPace, PollingActions};
use crate::ui::settings::mischief_tab;
use crate::ui::status_menu::{
    CharacterCount, StatusModel, TAG_ADD, TAG_DISMISS_ALL, TAG_HIDE, TAG_MISCHIEF, TAG_PAUSE,
    TAG_QUIT, TAG_REMOVE, TAG_SETTINGS, TAG_UPDATES,
};

use super::first_run;
use super::settings::SettingsWindow;

/// Decoded pack kept so adding another character does not import again.
pub struct PackCache {
    pub id: String,
    pub pack: Pack,
    pub images: Vec<DecodedImage>,
    pub sounds: Vec<DecodedWav>,
}

pub struct ExtraSlot {
    pub pack_id: String,
    pub character: String,
    pub runtime: Runtime<WindowHandle>,
    pub poll_actions: PollingActions,
    pub renderer: PanelRenderer,
    pub chase: String,
    pub toggles: Vec<(String, bool)>,
    pub clips: Clips,
    pub drag: Option<FrozenDrag>,
    pub shown: bool,
    /// Spawned ids that are not in `frames()` yet.
    pub awaiting_windows: Vec<MascotId>,
    /// Drop anchors to write back after `SpawnAt`.
    pub pending_drops: VecDeque<Point>,
    /// This shell paused the runtime so a drop's `MoveFrozen` lands first.
    pub drop_hold: bool,
}

pub fn install_shell(clock: &ShellClock) {
    super::status_menu::install(clock);
    super::first_run::AppDelegate::install(clock);
}

pub fn status_model(clock: &ShellClock) -> StatusModel {
    let state = clock.ivars().state.borrow();
    let characters = character_counts(&state);
    let live: usize = characters.iter().map(|row| row.live).sum();
    StatusModel {
        paused: state.power.user_paused(),
        hidden: state.power.hidden(),
        at_cap: live >= state.settings.mascot_cap,
        characters,
    }
}

pub fn status_act(clock: &ShellClock, tag: isize) {
    if tag == TAG_PAUSE {
        {
            let mut state = clock.ivars().state.borrow_mut();
            state.power.toggle_user_paused();
        }
        clock.sync_sources();
        return;
    }
    if tag == TAG_HIDE {
        {
            let mut state = clock.ivars().state.borrow_mut();
            state.power.toggle_hidden();
        }
        clock.sync_sources();
        return;
    }
    if tag == TAG_DISMISS_ALL {
        dismiss_everyone(clock);
        return;
    }
    if tag == TAG_MISCHIEF {
        let turn_on = clock.ivars().state.borrow().mischief.phase() == Phase::Off;
        set_mischief_enabled(clock, turn_on);
        return;
    }
    if tag == TAG_SETTINGS {
        show_settings(clock);
        return;
    }
    if tag == TAG_UPDATES {
        super::status_menu::open_releases();
        return;
    }
    if tag == TAG_QUIT {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        NSApplication::sharedApplication(mtm).terminate(None);
        return;
    }
    if (TAG_ADD..TAG_REMOVE).contains(&tag) {
        add_one(clock, (tag - TAG_ADD) as usize);
        return;
    }
    if tag >= TAG_REMOVE {
        remove_newest(clock, (tag - TAG_REMOVE) as usize);
    }
}

pub fn show_settings(clock: &ShellClock) {
    let window = {
        let mut slot = clock.ivars().settings_ui.borrow_mut();
        if slot.is_none() {
            *slot = Some(SettingsWindow::open(clock));
        }
        let ui = slot.as_mut().expect("settings window");
        ui.sync_from(clock);
        ui.agents.sync(clock);
        ui.window.clone()
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
}

pub fn login_act(sender: &AnyObject) {
    let state: NSControlStateValue = unsafe { objc2::msg_send![sender, state] };
    let on = state == NSControlStateValueOn;
    if let Err(error) = first_run::set_launch_at_login(on) {
        eprintln!("launch_at_login_error={error}");
        let revert = if on {
            NSControlStateValueOff
        } else {
            NSControlStateValueOn
        };
        unsafe {
            let _: () = objc2::msg_send![sender, setState: revert];
        }
        return;
    }
    println!(
        "launch_at_login={}",
        if first_run::launch_enabled() {
            "enabled"
        } else {
            "off"
        }
    );
}

pub fn settings_act(clock: &ShellClock, sender: &AnyObject) {
    let tag: isize = unsafe { objc2::msg_send![sender, tag] };
    if (40..=45).contains(&tag) {
        super::settings::agents_tab::act(clock, sender, tag);
        return;
    }
    if tag == 17 {
        super::import_flow::choose(clock);
        return;
    }
    if tag == 13 {
        super::settings::diagnostics_sheet::show(clock);
        return;
    }
    let mut rejected_hotkey = false;
    let mut hotkey_spec: Option<String> = None;
    let frozen = {
        let mut state = clock.ivars().state.borrow_mut();
        match tag {
            1 => {
                let value: f64 = unsafe { objc2::msg_send![sender, doubleValue] };
                state.settings.scaling = value;
            }
            2 => {
                let index: isize = unsafe { objc2::msg_send![sender, indexOfSelectedItem] };
                state.settings.filter = match index {
                    1 => ScaleFilter::Bicubic,
                    2 => ScaleFilter::Hqx,
                    _ => ScaleFilter::Nearest,
                };
            }
            3 => {
                let value: f64 = unsafe { objc2::msg_send![sender, doubleValue] };
                state.settings.opacity = value;
            }
            4 => state.settings.breeding = control_on(sender),
            5 => state.settings.transients = control_on(sender),
            6 => state.settings.transformation = control_on(sender),
            7 => state.settings.throwing = control_on(sender),
            8 => state.settings.sounds = control_on(sender),
            9 => state.settings.multiscreen = control_on(sender),
            14 => state.settings.keep_on_screen = control_on(sender),
            10 => {
                let text: objc2::rc::Retained<NSString> =
                    unsafe { objc2::msg_send![sender, stringValue] };
                let parsed: usize = super::lossy(&text).trim().parse().unwrap_or(0);
                let cap = parsed.clamp(1, shimeji_core::HARD_MASCOT_CAP);
                state.settings.mascot_cap = cap;
                let shown = NSString::from_str(&cap.to_string());
                unsafe {
                    let _: () = objc2::msg_send![sender, setStringValue: &*shown];
                }
            }
            11 => {
                let text: objc2::rc::Retained<NSString> =
                    unsafe { objc2::msg_send![sender, stringValue] };
                let spec = super::lossy(&text);
                let spec = spec.trim().to_string();
                match crate::platform::hotkey::parse(&spec) {
                    Ok(_) => hotkey_spec = Some(spec),
                    Err(error) => {
                        eprintln!("hotkey_parse={error}");
                        rejected_hotkey = true;
                        let revert = NSString::from_str(&state.settings.hotkey);
                        unsafe {
                            let _: () = objc2::msg_send![sender, setStringValue: &*revert];
                        }
                    }
                }
            }
            12 => {
                state.settings.animate_anyway = control_on(sender);
                let on = state.settings.animate_anyway;
                state.power.set_animate_anyway(on);
            }
            _ => {}
        }
        if rejected_hotkey {
            return;
        }
        let frozen = state.power.engine_frozen();
        apply_runtime_settings(&mut state);
        let dir = state.support_dir.clone();
        let mut stored = state.settings.clone();
        drop(state);
        if let Some(spec) = hotkey_spec {
            let chord = match crate::platform::hotkey::parse(&spec) {
                Ok(chord) => chord,
                Err(error) => {
                    eprintln!("hotkey_parse={error}");
                    return;
                }
            };
            let status = clock.install_hotkey(chord);
            println!("hotkey={chord}");
            println!("hotkey_status={status}");
            if status != 0 {
                let revert = NSString::from_str(&stored.hotkey);
                unsafe {
                    let _: () = objc2::msg_send![sender, setStringValue: &*revert];
                }
                return;
            }
            stored.hotkey = spec;
            clock.ivars().state.borrow_mut().settings.hotkey = stored.hotkey.clone();
        }
        if let Err(error) = settings::save(&dir, &stored) {
            eprintln!("settings_save={error}");
        }
        frozen
    };
    if matches!(tag, 1 | 3) {
        let state = clock.ivars().state.borrow();
        if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
            ui.sync_values(state.settings.scaling, state.settings.opacity);
        }
    }
    if tag == 8 {
        let on = clock.ivars().state.borrow().settings.sounds;
        if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
            ui.sync_sounds(on);
        }
    }
    clock.sync_sources();
    if frozen {
        clock.on_input();
    }
}

pub fn note_import(clock: &ShellClock, outcome: Outcome) {
    let (installed, kind) = match outcome {
        Outcome::Success(installed) => (installed, "success"),
        Outcome::Degraded(installed) => (installed, "degraded"),
        Outcome::Error { .. } | Outcome::Duplicate { .. } => return,
    };
    {
        let mut state = clock.ivars().state.borrow_mut();
        state.catalog.insert(
            &installed.id,
            &installed.path,
            &installed.pack,
            &installed.diagnostics,
        );
        let cache = PackCache {
            id: installed.id.clone(),
            pack: installed.pack,
            images: installed.images,
            sounds: installed.sounds,
        };
        if let Some(slot) = state.caches.iter_mut().find(|item| item.id == cache.id) {
            *slot = cache;
        } else {
            state.caches.push(cache);
        }
    }
    println!("import_cached={kind}");
    reload_pets(clock);
}

pub fn set_import_banner(clock: &ShellClock, text: &str) {
    if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
        ui.set_banner(text);
    }
}

pub fn context_menu(clock: &ShellClock, tag: isize) {
    let persist = {
        let mut state = clock.ivars().state.borrow_mut();
        let extra = state.menu_extra;
        let mascot = state.menu_mascot;
        match tag {
            context_menu::TAG_DISMISS => {
                dismiss_one(&mut state, extra, mascot);
                None
            }
            context_menu::TAG_DISMISS_ALL => {
                dismiss_slot(&mut state, extra);
                None
            }
            context_menu::TAG_FOLLOW => {
                follow(&mut state, extra);
                None
            }
            context_menu::TAG_PAUSE => {
                state.power.toggle_user_paused();
                None
            }
            tag if tag >= context_menu::TAG_TOGGLE => {
                let index = (tag - context_menu::TAG_TOGGLE) as usize;
                toggle_context(&mut state, extra, index)
            }
            _ => None,
        }
    };
    if let Some((dir, stored)) = persist {
        if let Err(error) = settings::save(&dir, &stored) {
            eprintln!("settings_save={error}");
        }
        reload_pets(clock);
    }
}

/// What the extra runtimes did on this frame, besides a context-menu click.
pub struct ExtraTick {
    pub menu: Option<(usize, MascotId)>,
    /// Sum of `frame.ticks` across extras. Zero means none of them ticked.
    pub ticks: u32,
    /// Window ids requested by extras that ticked.
    pub moved: Vec<u64>,
    /// Active carry actions, including extras that did not tick this callback.
    pub carrying: Vec<u64>,
    pub poll_pace: PollPace,
}

pub fn tick_extras(
    state: &mut AppState,
    space: &CocoaSpace,
    cursor: Point,
    now_ms: f64,
    from_timer: bool,
    allow_menu: bool,
) -> ExtraTick {
    if state.extras.is_empty() {
        return ExtraTick {
            menu: None,
            ticks: 0,
            moved: Vec::new(),
            carrying: Vec::new(),
            poll_pace: PollPace::Stopped,
        };
    }
    let frozen = state.power.engine_frozen();
    let multiscreen = state.settings.multiscreen;
    let pets_shown = state.pets_shown;
    let snapshot = Snapshot {
        screens: space.snapshot_screens(),
        cursor,
        active_window: state.poller.slot.load_sample(),
        multiscreen,
    };
    let mut menu = None;
    let mut sound_batches = Vec::new();
    let mut tick_count = 0u32;
    let mut poll_pace = PollPace::Stopped;
    let mut moved = Vec::new();
    let mut carrying = Vec::new();
    {
        let extras = &mut state.extras;
        let mouse_events = &mut state.mouse_events;
        let ticks = &mut state.ticks;
        for (index, extra) in extras.iter_mut().enumerate() {
            let mut clicked = None;
            let events = extra.renderer.input().drain();
            for event in events {
                *mouse_events += 1;
                match event {
                    InputEvent::LeftDown(id) if frozen => {
                        if let Some(frame) = extra
                            .runtime
                            .frames()
                            .into_iter()
                            .find(|item| item.id == id)
                        {
                            extra.drag = Some(FrozenDrag {
                                id,
                                dx: frame.anchor.x - cursor.x,
                                dy: frame.anchor.y - cursor.y,
                            });
                        }
                    }
                    InputEvent::LeftDown(id) => {
                        extra.drag = None;
                        extra.runtime.push(Command::Press {
                            mascot: Some(id),
                            button: shimeji_core::MouseButton::Left,
                        });
                    }
                    InputEvent::LeftUp(id) if frozen => {
                        if extra.drag.as_ref().is_some_and(|drag| drag.id == id) {
                            extra.drag = None;
                        }
                    }
                    InputEvent::LeftUp(id) => {
                        extra.runtime.push(Command::Release {
                            mascot: Some(id),
                            button: shimeji_core::MouseButton::Left,
                        });
                    }
                    InputEvent::Drag(id) if frozen => {
                        let offset = extra
                            .drag
                            .filter(|drag| drag.id == id)
                            .map(|drag| (drag.dx, drag.dy));
                        if let Some((dx, dy)) = offset {
                            extra.runtime.push(Command::MoveFrozen {
                                id,
                                anchor: Point::new(cursor.x + dx, cursor.y + dy),
                            });
                        }
                    }
                    InputEvent::Drag(_) => {}
                    InputEvent::RightDown(id) => {
                        if allow_menu {
                            clicked = Some(id);
                        }
                    }
                }
            }
            let frame = extra.runtime.on_frame(now_ms, &snapshot);
            if frame.ticks > 0 {
                tick_count = tick_count.saturating_add(frame.ticks);
                moved.extend(frame.window_moves.iter().map(|item| item.id));
            }
            if from_timer {
                *ticks += u64::from(frame.ticks);
            }
            let spawned: Vec<MascotId> = frame.spawned.to_vec();
            let sounds: Vec<(String, Option<String>)> = frame
                .sounds
                .iter()
                .map(|event| (event.name.clone(), event.volume.clone()))
                .collect();
            let mut merged = Vec::new();
            for tick in frame.deltas {
                for delta in &tick.mascots {
                    merge_delta(&mut merged, *delta);
                }
            }
            for delta in frame.frozen {
                merge_delta(&mut merged, *delta);
            }
            let ticked = frame.ticks > 0;
            restore_slot(extra, &spawned, !frozen);
            let live = extra.runtime.frames();
            carrying.extend(
                extra
                    .poll_actions
                    .carry_windows(&live, snapshot.active_window.as_ref()),
            );
            poll_pace = poll_pace.merge(extra.poll_actions.pace(
                true,
                &live,
                snapshot.active_window.as_ref(),
            ));
            let ready = take_ready(&mut extra.awaiting_windows, &spawned, ticked, |id| {
                live.iter().any(|item| item.id == id)
            });
            for id in ready {
                if let Some(mascot) = live.iter().find(|item| item.id == id) {
                    extra.renderer.attach(id, mascot);
                }
            }
            let attached = extra.renderer.live_ids();
            for id in attached {
                if live.iter().all(|item| item.id != id) {
                    extra.renderer.detach(id);
                }
            }
            extra.renderer.commit(&live, &merged);
            if !pets_shown {
                extra.renderer.hide_all();
                extra.shown = false;
            }
            sound_batches.push(sounds);
            if let Some(id) = clicked {
                menu = Some((index, id));
            }
        }
    }
    for (index, sounds) in sound_batches.into_iter().enumerate() {
        play_extra(state, index, &sounds);
    }
    ExtraTick {
        menu,
        ticks: tick_count,
        moved,
        carrying,
        poll_pace,
    }
}

pub fn sync_extras(state: &mut AppState) {
    let clock_on = state.power.sources().clock;
    let frozen = state.power.engine_frozen();
    let visible = state.power.pets_visible();
    for extra in &mut state.extras {
        if extra.runtime.paused() != frozen {
            extra.runtime.set_paused(frozen);
        }
        extra.renderer.input().set_deliver_now(!clock_on);
        if visible {
            if !extra.shown {
                extra.renderer.show_all();
                extra.shown = true;
            }
        } else if extra.shown {
            extra.renderer.hide_all();
            extra.shown = false;
        }
    }
}

pub fn refresh_extra_screens(state: &mut AppState, space: &CocoaSpace) {
    for extra in &mut state.extras {
        extra.renderer.set_space(space.clone());
    }
}

pub fn sum_extra_counters(state: &AppState, totals: &mut crate::platform::counters::Totals) {
    for extra in &state.extras {
        totals.window_moves += extra.renderer.counters.window_moves;
        totals.layer_moves += extra.renderer.counters.layer_moves;
        totals.contents_sets += extra.renderer.counters.contents_sets;
        totals.decoded_image_bytes += extra.renderer.decoded_bytes();
    }
}

fn character_counts(state: &AppState) -> Vec<CharacterCount> {
    let mut rows = Vec::new();
    for pack in state.catalog.packs() {
        for character in &pack.characters {
            let live = live_count(state, &pack.id, &character.name);
            rows.push(CharacterCount {
                pack: pack.id.clone(),
                name: character.name.clone(),
                live,
                enabled: state.settings.character_enabled(&pack.id, &character.name),
            });
        }
    }
    rows
}

fn live_count(state: &AppState, pack: &str, name: &str) -> usize {
    if is_primary(state, pack, name) {
        return state.renderer.live_ids().len();
    }
    state
        .extras
        .iter()
        .find(|extra| extra.pack_id == pack && extra.character == name)
        .map(|extra| extra.renderer.live_ids().len())
        .unwrap_or(0)
}

fn is_primary(state: &AppState, pack: &str, name: &str) -> bool {
    state.pack_id == pack && state.character_name == name
}

fn add_one(clock: &ShellClock, index: usize) {
    let target = {
        let state = clock.ivars().state.borrow();
        let rows = character_counts(&state);
        let Some(row) = rows.get(index) else {
            return;
        };
        if !row.enabled {
            return;
        }
        let live: usize = rows.iter().map(|item| item.live).sum();
        if live >= state.settings.mascot_cap {
            return;
        }
        (row.pack.clone(), row.name.clone())
    };
    let (pack, name) = target;
    let primary = {
        let state = clock.ivars().state.borrow();
        is_primary(&state, &pack, &name)
    };
    if !primary && !ensure_extra(clock, &pack, &name) {
        return;
    }
    let frozen = clock.ivars().state.borrow().power.engine_frozen();
    let anchor = if frozen {
        floor_point()
    } else {
        fresh_drop(clock, &pack, &name)
    };
    {
        let mut state = clock.ivars().state.borrow_mut();
        if primary {
            queue_primary(&mut state, anchor, !frozen);
        } else if let Some(extra) = state
            .extras
            .iter_mut()
            .find(|extra| extra.pack_id == pack && extra.character == name)
        {
            queue_slot(extra, anchor, !frozen);
        }
    }
    if frozen {
        clock.on_input();
    }
}

fn remove_newest(clock: &ShellClock, index: usize) {
    let target = {
        let state = clock.ivars().state.borrow();
        let rows = character_counts(&state);
        let Some(row) = rows.get(index) else {
            return;
        };
        (row.pack.clone(), row.name.clone())
    };
    let mut state = clock.ivars().state.borrow_mut();
    if is_primary(&state, &target.0, &target.1) {
        if let Some(id) = newest(&state.renderer.live_ids()) {
            state.renderer.detach(id);
            state.runtime.push(Command::Dismiss(id));
        }
        return;
    }
    if let Some(extra) = state
        .extras
        .iter_mut()
        .find(|extra| extra.pack_id == target.0 && extra.character == target.1)
    {
        if let Some(id) = newest(&extra.renderer.live_ids()) {
            extra.renderer.detach(id);
            extra.runtime.push(Command::Dismiss(id));
        }
    }
}

fn dismiss_everyone(clock: &ShellClock) {
    let mut state = clock.ivars().state.borrow_mut();
    let ids = state.renderer.live_ids();
    for id in ids {
        state.renderer.detach(id);
        state.runtime.push(Command::Dismiss(id));
    }
    for extra in &mut state.extras {
        let ids = extra.renderer.live_ids();
        for id in ids {
            extra.renderer.detach(id);
            extra.runtime.push(Command::Dismiss(id));
        }
    }
}

pub fn set_character_enabled(clock: &ShellClock, pack: &str, name: &str, enabled: bool) {
    let saved = {
        let mut state = clock.ivars().state.borrow_mut();
        state.settings.set_character_enabled(pack, name, enabled);
        if !enabled {
            if is_primary(&state, pack, name) {
                let ids = state.renderer.live_ids();
                for id in ids {
                    state.renderer.detach(id);
                    state.runtime.push(Command::Dismiss(id));
                }
            } else if let Some(extra) = state
                .extras
                .iter_mut()
                .find(|extra| extra.pack_id == pack && extra.character == name)
            {
                let ids = extra.renderer.live_ids();
                for id in ids {
                    extra.renderer.detach(id);
                    extra.runtime.push(Command::Dismiss(id));
                }
            }
        }
        (state.support_dir.clone(), state.settings.clone())
    };
    if let Err(error) = settings::save(&saved.0, &saved.1) {
        eprintln!("settings_save={error}");
    }
}

pub fn set_behavior_enabled(
    clock: &ShellClock,
    pack: &str,
    character: &str,
    behavior: &str,
    enabled: bool,
) {
    let saved = {
        let mut state = clock.ivars().state.borrow_mut();
        state
            .settings
            .set_behavior_enabled(pack, character, behavior, enabled);
        push_behavior(&mut state, pack, character, behavior, enabled);
        (state.support_dir.clone(), state.settings.clone())
    };
    if let Err(error) = settings::save(&saved.0, &saved.1) {
        eprintln!("settings_save={error}");
    }
}

fn push_behavior(state: &mut AppState, pack: &str, character: &str, behavior: &str, enabled: bool) {
    let command = Command::SetBehaviorEnabled {
        name: behavior.to_string(),
        enabled,
    };
    if is_primary(state, pack, character) {
        if let Some((_, flag)) = state.toggles.iter_mut().find(|(name, _)| name == behavior) {
            *flag = enabled;
        }
        state.runtime.push(command);
        return;
    }
    if let Some(extra) = state
        .extras
        .iter_mut()
        .find(|extra| extra.pack_id == pack && extra.character == character)
    {
        if let Some((_, flag)) = extra.toggles.iter_mut().find(|(name, _)| name == behavior) {
            *flag = enabled;
        }
        extra.runtime.push(command);
    }
}

fn toggle_context(
    state: &mut AppState,
    extra: Option<usize>,
    index: usize,
) -> Option<(std::path::PathBuf, Settings)> {
    let (pack, character, toggles_name, enabled) = if let Some(slot) = extra {
        let extra = state.extras.get_mut(slot)?;
        let (name, flag) = extra.toggles.get_mut(index)?;
        *flag = !*flag;
        let enabled = *flag;
        let name = name.clone();
        extra.runtime.push(Command::SetBehaviorEnabled {
            name: name.clone(),
            enabled,
        });
        (
            extra.pack_id.clone(),
            extra.character.clone(),
            name,
            enabled,
        )
    } else {
        let (name, flag) = state.toggles.get_mut(index)?;
        *flag = !*flag;
        let enabled = *flag;
        let name = name.clone();
        state.runtime.push(Command::SetBehaviorEnabled {
            name: name.clone(),
            enabled,
        });
        (
            state.pack_id.clone(),
            state.character_name.clone(),
            name,
            enabled,
        )
    };
    state
        .settings
        .set_behavior_enabled(&pack, &character, &toggles_name, enabled);
    Some((state.support_dir.clone(), state.settings.clone()))
}

fn dismiss_one(state: &mut AppState, extra: Option<usize>, mascot: Option<MascotId>) {
    let Some(id) = mascot else {
        return;
    };
    if let Some(slot) = extra {
        if let Some(extra) = state.extras.get_mut(slot) {
            extra.renderer.detach(id);
            extra.runtime.push(Command::Dismiss(id));
        }
        return;
    }
    state.renderer.detach(id);
    state.runtime.push(Command::Dismiss(id));
}

fn dismiss_slot(state: &mut AppState, extra: Option<usize>) {
    if let Some(slot) = extra {
        if let Some(extra) = state.extras.get_mut(slot) {
            let ids = extra.renderer.live_ids();
            for id in ids {
                extra.renderer.detach(id);
                extra.runtime.push(Command::Dismiss(id));
            }
        }
        return;
    }
    let ids = state.renderer.live_ids();
    for id in ids {
        state.renderer.detach(id);
        state.runtime.push(Command::Dismiss(id));
    }
}

fn follow(state: &mut AppState, extra: Option<usize>) {
    if let Some(slot) = extra {
        if let Some(extra) = state.extras.get_mut(slot) {
            let name = extra.chase.clone();
            extra.runtime.push(Command::SetBehavior { id: None, name });
        }
        return;
    }
    let name = state.chase.clone();
    state.runtime.push(Command::SetBehavior { id: None, name });
}

fn ensure_extra(clock: &ShellClock, pack_id: &str, name: &str) -> bool {
    if clock
        .ivars()
        .state
        .borrow()
        .extras
        .iter()
        .any(|extra| extra.pack_id == pack_id && extra.character == name)
    {
        return true;
    }
    if !cache_pack(clock, pack_id) {
        return false;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let space = read_space(mtm);
    let built = {
        let state = clock.ivars().state.borrow();
        let Some(cache) = state.caches.iter().find(|item| item.id == pack_id) else {
            return false;
        };
        let Some(character) = cache.pack.characters.iter().find(|item| item.name == name) else {
            return false;
        };
        Some(build_slot(
            mtm,
            &state.settings,
            cache,
            character,
            &space,
            ExtraLaunch {
                seed: state.seed.wrapping_add(state.extras.len() as i64 + 1),
                slack: state.slack,
                pets_shown: state.pets_shown,
                clock_on: state.power.sources().clock,
                windows: state.mischief.handle(),
            },
        ))
    };
    let Some(slot) = built else {
        return false;
    };
    bind_extra_wake(clock, slot.renderer.input());
    clock.ivars().state.borrow_mut().extras.push(slot);
    true
}

fn cache_pack(clock: &ShellClock, pack_id: &str) -> bool {
    if clock
        .ivars()
        .state
        .borrow()
        .caches
        .iter()
        .any(|item| item.id == pack_id)
    {
        return true;
    }
    let path = {
        let state = clock.ivars().state.borrow();
        let Some(pack) = state.catalog.get(pack_id) else {
            return false;
        };
        pack.path.clone()
    };
    let root = clock.ivars().state.borrow().catalog.root().to_path_buf();
    let outcome =
        shimeji_app::import::import(&path, &root, Some(shimeji_app::import::Choice::Replace));
    let installed = match outcome {
        Outcome::Success(installed) | Outcome::Degraded(installed) => installed,
        Outcome::Error { message } => {
            eprintln!("import_pixels={message}");
            return false;
        }
        Outcome::Duplicate { id } => {
            eprintln!("import_pixels=duplicate {id}");
            return false;
        }
    };
    let mut state = clock.ivars().state.borrow_mut();
    state.catalog.insert(
        &installed.id,
        &installed.path,
        &installed.pack,
        &installed.diagnostics,
    );
    state.caches.push(PackCache {
        id: installed.id,
        pack: installed.pack,
        images: installed.images,
        sounds: installed.sounds,
    });
    true
}

struct ExtraLaunch {
    seed: i64,
    slack: u32,
    pets_shown: bool,
    clock_on: bool,
    windows: WindowHandle,
}

fn build_slot(
    mtm: MainThreadMarker,
    settings: &Settings,
    cache: &PackCache,
    character: &Character,
    space: &CocoaSpace,
    launch: ExtraLaunch,
) -> ExtraSlot {
    let poses = PoseTable::new(character, &cache.images);
    let images = ImageCache::from_import(character, &cache.images);
    let queue = InputQueue::default();
    queue.set_deliver_now(!launch.clock_on);
    let mut renderer = PanelRenderer::new(mtm, images, poses, space.clone(), queue, launch.slack);
    apply_visuals(&mut renderer, settings);
    let mut runtime = Runtime::with_windows(character, launch.seed, launch.windows);
    runtime.push(Command::Settings(settings.engine_settings()));
    runtime.push(Command::SetMascotCap(settings.mascot_cap));
    for (behavior, _) in character.behaviors.iter() {
        if character
            .behaviors
            .get(behavior)
            .is_some_and(|item| item.toggleable)
            && !settings.behavior_enabled(&cache.id, &character.name, behavior)
        {
            runtime.push(Command::SetBehaviorEnabled {
                name: behavior.to_string(),
                enabled: false,
            });
        }
    }
    let chase = character
        .behavior_schema
        .required_behaviors()
        .first()
        .copied()
        .unwrap_or("ChaseMouse")
        .to_string();
    let toggles = menu_toggles(settings, &cache.id, character);
    let mut clips = Clips::default();
    for wav in &cache.sounds {
        clips.insert(
            &wav.path,
            sound::Clip {
                channels: wav.channels,
                sample_rate: wav.sample_rate,
                bits_per_sample: wav.bits_per_sample,
                pcm: wav.pcm.clone(),
            },
        );
    }
    ExtraSlot {
        pack_id: cache.id.clone(),
        character: character.name.clone(),
        runtime,
        poll_actions: PollingActions::new(character),
        renderer,
        chase,
        toggles,
        clips,
        drag: None,
        shown: launch.pets_shown,
        awaiting_windows: Vec::new(),
        pending_drops: VecDeque::new(),
        drop_hold: false,
    }
}

fn menu_toggles(settings: &Settings, pack: &str, character: &Character) -> Vec<(String, bool)> {
    character
        .behaviors
        .iter()
        .filter(|(_, behavior)| behavior.toggleable && !behavior.hidden)
        .map(|(name, _)| {
            (
                name.to_string(),
                settings.behavior_enabled(pack, &character.name, name),
            )
        })
        .collect()
}

fn bind_extra_wake(clock: &ShellClock, queue: &InputQueue) {
    let ptr = std::ptr::from_ref(clock) as usize;
    queue.set_wake(Arc::new(move || {
        let clock = unsafe { &*(ptr as *const ShellClock) };
        clock.on_input();
    }));
}

fn apply_runtime_settings(state: &mut AppState) {
    let settings = state.settings.clone();
    apply_visuals(&mut state.renderer, &settings);
    state
        .runtime
        .push(Command::Settings(settings.engine_settings()));
    state
        .runtime
        .push(Command::SetMascotCap(settings.mascot_cap));
    for extra in &mut state.extras {
        apply_visuals(&mut extra.renderer, &settings);
        extra
            .runtime
            .push(Command::Settings(settings.engine_settings()));
        extra
            .runtime
            .push(Command::SetMascotCap(settings.mascot_cap));
    }
    state.mute.enabled = settings.sounds;
    if settings.sounds && state.player.is_none() {
        state.player = Some(sound::Player::new());
    }
}

pub fn apply_visuals(renderer: &mut PanelRenderer, settings: &Settings) {
    renderer.set_user_scale(settings.scaling);
    renderer.set_opacity(settings.opacity);
    renderer.set_filter_linear(matches!(settings.filter, ScaleFilter::Bicubic));
    renderer.set_keep_on_screen(settings.keep_on_screen);
}

fn floor_point() -> Option<Point> {
    let mtm = MainThreadMarker::new()?;
    first_run::main_floor(&read_space(mtm))
}

/// Spawn one of `pack`/`name` when agent reactions need a pet and none is live.
pub(crate) fn spawn_character(clock: &ShellClock, pack: &str, name: &str) {
    let primary = {
        let state = clock.ivars().state.borrow();
        is_primary(&state, pack, name)
    };
    if !primary && !ensure_extra(clock, pack, name) {
        return;
    }
    let frozen = clock.ivars().state.borrow().power.engine_frozen();
    let anchor = if frozen {
        floor_point()
    } else {
        fresh_drop(clock, pack, name)
    };
    let mut state = clock.ivars().state.borrow_mut();
    if primary {
        queue_primary(&mut state, anchor, !frozen);
    } else if let Some(extra) = state
        .extras
        .iter_mut()
        .find(|extra| extra.pack_id == pack && extra.character == name)
    {
        queue_slot(extra, anchor, !frozen);
    }
}

/// Queue a spawn. A falling drop is paused for one frame so `MoveFrozen` can
/// put the anchor back before the first tick. `hold` is false for the floor
/// placement, which the engine keeps.
fn queue_primary(state: &mut AppState, anchor: Option<Point>, hold: bool) {
    let Some(anchor) = anchor else {
        state.runtime.push(Command::Spawn);
        return;
    };
    if hold {
        let already = state.runtime.paused();
        if !already && !state.drop_hold {
            state.drop_hold = true;
            state.runtime.set_paused(true);
        }
        state.pending_drops.push_back(anchor);
    }
    state.runtime.push(Command::SpawnAt(anchor));
}

fn queue_slot(extra: &mut ExtraSlot, anchor: Option<Point>, hold: bool) {
    let Some(anchor) = anchor else {
        extra.runtime.push(Command::Spawn);
        return;
    };
    if hold {
        let already = extra.runtime.paused();
        if !already && !extra.drop_hold {
            extra.drop_hold = true;
            extra.runtime.set_paused(true);
        }
        extra.pending_drops.push_back(anchor);
    }
    extra.runtime.push(Command::SpawnAt(anchor));
}

/// Write drop anchors back onto the ids `SpawnAt` just reported.
pub(crate) fn restore_primary(state: &mut AppState, spawned: &[MascotId]) {
    let unpause = !state.power.engine_frozen();
    let had = !state.pending_drops.is_empty();
    let fixups = take_fixups(&mut state.pending_drops, spawned);
    if had {
        state.pending_drops.clear();
    }
    let release = had && state.drop_hold;
    if release {
        state.drop_hold = false;
    }
    for (id, anchor) in fixups {
        state.runtime.push(Command::MoveFrozen { id, anchor });
    }
    if release && unpause {
        state.runtime.set_paused(false);
    }
}

fn restore_slot(extra: &mut ExtraSlot, spawned: &[MascotId], unpause: bool) {
    let ExtraSlot {
        runtime,
        pending_drops,
        drop_hold,
        ..
    } = extra;
    restore_fields(runtime, pending_drops, drop_hold, spawned, unpause);
}

fn restore_fields<W: WindowControl>(
    runtime: &mut Runtime<W>,
    pending: &mut VecDeque<Point>,
    drop_hold: &mut bool,
    spawned: &[MascotId],
    unpause: bool,
) {
    let had = !pending.is_empty();
    let fixups = take_fixups(pending, spawned);
    if had {
        pending.clear();
    }
    let release = had && *drop_hold;
    if release {
        *drop_hold = false;
    }
    for (id, anchor) in fixups {
        runtime.push(Command::MoveFrozen { id, anchor });
    }
    if release && unpause {
        runtime.set_paused(false);
    }
}

/// Status-menu Add, when the engine is running. Draws the shell RNG.
fn fresh_drop(clock: &ShellClock, pack: &str, name: &str) -> Option<Point> {
    let mtm = MainThreadMarker::new()?;
    let work = first_run::primary_work(&read_space(mtm))?;
    let mut state = clock.ivars().state.borrow_mut();
    let headroom = if is_primary(&state, pack, name) {
        state.renderer.drop_headroom()
    } else {
        state
            .extras
            .iter()
            .find(|extra| extra.pack_id == pack && extra.character == name)?
            .renderer
            .drop_headroom()
    };
    let window = state.poller.slot.load_sample();
    let anchor = spawn_anchor(work, &mut state.drop_rng);
    Some(visible_drop_anchor(anchor, work, window.as_ref(), headroom))
}

fn newest(ids: &[MascotId]) -> Option<MascotId> {
    ids.iter().copied().max_by_key(|id| id.0)
}

pub(crate) fn sync_mischief_chrome(clock: &ShellClock) {
    super::mischief_prompt::sync(clock);
    let (phase, enabled) = {
        let state = clock.ivars().state.borrow();
        (state.mischief.phase(), state.settings.mischief)
    };
    if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
        ui.sync_mischief_status(phase, enabled);
    }
}

pub fn mischief_act(clock: &ShellClock, sender: &AnyObject) {
    let tag: isize = unsafe { objc2::msg_send![sender, tag] };
    if tag == mischief_tab::TAG_ENABLE {
        set_mischief_enabled(clock, control_on(sender));
        return;
    }
    if (mischief_tab::TAG_DEFAULT..mischief_tab::TAG_DEFAULT + DEFAULT_BLOCKED_APPS.len() as isize)
        .contains(&tag)
    {
        let index = (tag - mischief_tab::TAG_DEFAULT) as usize;
        let Some(name) = DEFAULT_BLOCKED_APPS.get(index) else {
            return;
        };
        let mut rules = clock.ivars().state.borrow().settings.window_rules.clone();
        if !rules.set_default_allowed(name, control_on(sender)) {
            if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
                ui.sync_mischief_rules(&rules);
            }
            return;
        }
        commit_window_rules(clock, rules);
        return;
    }
    if tag == mischief_tab::TAG_ADD {
        let Some((pattern, target, effect)) =
            clock.ivars().settings_ui.borrow().as_ref().map(|ui| {
                (
                    ui.rule_pattern(),
                    ui.rule_target_index(),
                    ui.rule_effect_index(),
                )
            })
        else {
            return;
        };
        let mut rules = clock.ivars().state.borrow().settings.window_rules.clone();
        let target = if target == 0 {
            Target::App
        } else {
            Target::Title
        };
        let effect = if effect == 0 {
            Effect::Allow
        } else {
            Effect::Block
        };
        if !rules.add_rule(effect, target, &pattern) {
            return;
        }
        if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
            ui.clear_rule_pattern();
        }
        commit_window_rules(clock, rules);
        return;
    }
    if tag == mischief_tab::TAG_REMOVE {
        let index = clock
            .ivars()
            .settings_ui
            .borrow()
            .as_ref()
            .map(|ui| ui.selected_rule_index())
            .unwrap_or(-1);
        if index < 0 {
            return;
        }
        let mut rules = clock.ivars().state.borrow().settings.window_rules.clone();
        if rules.rules.is_empty() || !rules.remove_rule(index as usize) {
            return;
        }
        commit_window_rules(clock, rules);
    }
}

pub fn mischief_prompt_act(clock: &ShellClock, sender: &AnyObject) {
    let tag: isize = unsafe { objc2::msg_send![sender, tag] };
    if tag == super::mischief_prompt::TAG_OPEN {
        crate::mischief::open_accessibility_pane();
        return;
    }
    if tag == super::mischief_prompt::TAG_NOT_NOW {
        dismiss_mischief_explanation(clock);
    }
}

fn set_mischief_enabled(clock: &ShellClock, on: bool) {
    let stored = {
        let mut state = clock.ivars().state.borrow_mut();
        state.settings.mischief = on;
        state.mischief.set_enabled(on);
        (state.support_dir.clone(), state.settings.clone())
    };
    if let Err(error) = settings::save(&stored.0, &stored.1) {
        eprintln!("settings_save={error}");
    }
    sync_mischief_chrome(clock);
}

fn dismiss_mischief_explanation(clock: &ShellClock) {
    let stored = {
        let mut state = clock.ivars().state.borrow_mut();
        let was = state.mischief.phase();
        state.mischief.dismiss_explanation();
        if was.shows_explanation() && state.mischief.phase() == Phase::Off {
            state.settings.mischief = false;
        }
        (state.support_dir.clone(), state.settings.clone())
    };
    if let Err(error) = settings::save(&stored.0, &stored.1) {
        eprintln!("settings_save={error}");
    }
    sync_mischief_chrome(clock);
}

fn commit_window_rules(clock: &ShellClock, mut rules: WindowRules) {
    rules.sanitize();
    let stored = {
        let mut state = clock.ivars().state.borrow_mut();
        state.settings.window_rules = rules.clone();
        let view = state.mischief.handle().snapshot();
        state
            .mischief
            .publish(view.pid, view.window, view.bounds, view.screens, rules);
        (state.support_dir.clone(), state.settings.clone())
    };
    if let Err(error) = settings::save(&stored.0, &stored.1) {
        eprintln!("settings_save={error}");
    }
    if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
        ui.sync_mischief_rules(&stored.1.window_rules);
    }
}

fn control_on(sender: &AnyObject) -> bool {
    let state: NSControlStateValue = unsafe { objc2::msg_send![sender, state] };
    state == NSControlStateValueOn
}

fn play_extra(state: &mut AppState, index: usize, sounds: &[(String, Option<String>)]) {
    if !state.mute.allows() || sounds.is_empty() {
        return;
    }
    let frames: Vec<sound::FloatFrames> = {
        let Some(extra) = state.extras.get(index) else {
            return;
        };
        sounds
            .iter()
            .filter_map(|(name, volume)| {
                let clip = extra.clips.get(name)?;
                Some(sound::to_float_frames(
                    clip,
                    sound::gain_from_volume(volume.as_deref()),
                ))
            })
            .collect()
    };
    if let Some(player) = state.player.as_mut() {
        for frame in &frames {
            player.play_frames(frame);
        }
    }
}

fn merge_delta(merged: &mut Vec<shimeji_core::MascotDelta>, delta: shimeji_core::MascotDelta) {
    if let Some(existing) = merged.iter_mut().find(|item| item.id == delta.id) {
        existing.anchor_changed |= delta.anchor_changed;
        existing.image_changed |= delta.image_changed;
    } else {
        merged.push(delta);
    }
}

pub fn reload_pets(clock: &ShellClock) {
    if let Some(ui) = clock.ivars().settings_ui.borrow().as_ref() {
        ui.reload_outline();
    }
}

fn push_initial_disabled(
    runtime: &mut Runtime<WindowHandle>,
    settings: &Settings,
    pack: &str,
    character: &Character,
) {
    for (name, behavior) in character.behaviors.iter() {
        if behavior.toggleable && !settings.behavior_enabled(pack, &character.name, name) {
            runtime.push(Command::SetBehaviorEnabled {
                name: name.to_string(),
                enabled: false,
            });
        }
    }
}

/// Startup helper used by `app` so the primary slot honors saved toggles.
pub fn prepare_primary(
    runtime: &mut Runtime<WindowHandle>,
    renderer: &mut PanelRenderer,
    settings: &Settings,
    pack: &str,
    character: &Character,
) -> Vec<(String, bool)> {
    apply_visuals(renderer, settings);
    runtime.push(Command::Settings(settings.engine_settings()));
    runtime.push(Command::SetMascotCap(settings.mascot_cap));
    push_initial_disabled(runtime, settings, pack, character);
    menu_toggles(settings, pack, character)
}
