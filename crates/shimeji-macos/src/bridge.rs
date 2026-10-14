//! AppKit object that owns the runtime and receives the GCD tick.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSApplication, NSApplicationDidBecomeActiveNotification,
    NSApplicationDidChangeOcclusionStateNotification,
    NSApplicationDidChangeScreenParametersNotification, NSApplicationOcclusionState, NSEvent,
    NSMenu, NSMenuDelegate, NSPopover, NSStatusItem, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidWakeNotification,
    NSWorkspaceScreensDidSleepNotification, NSWorkspaceScreensDidWakeNotification,
    NSWorkspaceSessionDidBecomeActiveNotification, NSWorkspaceSessionDidResignActiveNotification,
    NSWorkspaceWillSleepNotification,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSString,
};
use shimeji_app::journal::Bounds;
use shimeji_app::mischief_state::Phase;
use shimeji_app::{Command, Runtime};
use shimeji_core::JavaRandom;
use shimeji_core::delta::{MascotDelta, MascotId};
use shimeji_core::engine::MouseButton;
use shimeji_core::env::{Point, Snapshot};

use crate::mischief::{Session, WindowHandle};
use crate::platform::clock::GcdTimer;
use crate::platform::context_menu::{self, MenuModel};
use crate::platform::counters::{self, Totals};
use crate::platform::hotkey::{self, Chord};
use crate::platform::input::InputEvent;
use crate::platform::panel::PanelRenderer;
use crate::platform::power::Power;
use crate::platform::render::Renderer;
use crate::platform::screens::{engine_screen_rects, main_display_id, read_space};
use crate::platform::sound::{self, Library, Mute, Player};
use crate::platform::spawn::take_ready;
use crate::platform::window_env::{PollPace, PollingActions};
use crate::threads::poller::Poller;
use crate::ui::actions::{ExtraSlot, PackCache};
use crate::ui::first_run::AppDelegate;
use crate::ui::settings::SettingsWindow;

const STEP_MS: u64 = 40;
const FALL_JA: &str = "落下する";
const FALL_EN: &str = "Fall";

#[derive(Clone, Copy)]
pub(crate) struct FrozenDrag {
    pub(crate) id: MascotId,
    pub(crate) dx: i32,
    pub(crate) dy: i32,
}

pub struct AppState {
    pub runtime: Runtime<WindowHandle>,
    pub renderer: PanelRenderer,
    pub poller: Poller,
    pub poll_actions: PollingActions,
    pub our_pid: i32,
    pub origin: Instant,
    pub ticks: u64,
    pub resyncs: u64,
    pub mouse_events: u64,
    pub main_display: Option<(i32, i32, i32, i32)>,
    pub power: Power,
    pub chase: String,
    pub toggles: Vec<(String, bool)>,
    pub menu_mascot: Option<MascotId>,
    pub drag: Option<FrozenDrag>,
    pub library: Library,
    pub mute: Mute,
    pub player: Option<Player>,
    pub spawn_target: usize,
    pub walk: Option<String>,
    pub settle_pause: bool,
    pub settled: bool,
    /// Timer wakeups at the tick that finished arming and paused (S5).
    pub measure_base: Option<u64>,
    pub pets_shown: bool,
    /// Occlusion is ignored until a shown window has once reported visible,
    /// so our own `orderOut` cannot stick the asleep flag.
    pub occlusion_trusted: bool,
    pub hotkey: Option<hotkey::Registration>,
    pub support_dir: std::path::PathBuf,
    pub settings: shimeji_app::settings::Settings,
    pub catalog: shimeji_app::library::Library,
    pub pack_id: String,
    pub character_name: String,
    pub extras: Vec<ExtraSlot>,
    pub menu_extra: Option<usize>,
    pub caches: Vec<PackCache>,
    pub seed: i64,
    /// Shell RNG for launch and menu spawns. Seeded from `seed`.
    pub drop_rng: JavaRandom,
    /// `first_x=` has already been printed for this process.
    pub announced_first_x: bool,
    /// Spawned ids that are not in `frames()` yet.
    pub awaiting_windows: Vec<MascotId>,
    /// Drop anchors to write back after `SpawnAt`. See [`take_fixups`].
    pub pending_drops: VecDeque<Point>,
    /// This shell paused the runtime so a drop's `MoveFrozen` lands first.
    pub drop_hold: bool,
    pub slack: u32,
    pub mischief: Session,
    pub agent: crate::agent::react::Session,
}

pub struct ShellIvars {
    pub state: RefCell<AppState>,
    pub screens_dirty: AtomicBool,
    pub app_activated: AtomicBool,
    pub self_activated: AtomicBool,
    pub timer: RefCell<Option<GcdTimer>>,
    pub timer_wakeups: Cell<u64>,
    pub late_ticks: Cell<u64>,
    pub last_callback: Cell<Option<Instant>>,
    pub menu_open: Cell<bool>,
    /// Opt-in local input/position trace for reproducing motion glitches.
    pub motion_trace: bool,
    pub status_item: RefCell<Option<Retained<NSStatusItem>>>,
    pub status_menu: RefCell<Option<Retained<NSMenu>>>,
    pub delegate: RefCell<Option<Retained<AppDelegate>>>,
    pub settings_ui: RefCell<Option<SettingsWindow>>,
    pub popover: RefCell<Option<Retained<NSPopover>>>,
    pub diagnostics_alert: RefCell<Option<Retained<NSAlert>>>,
    pub mischief_prompt: RefCell<Option<crate::ui::mischief_prompt::Prompt>>,
    pub badge: RefCell<Option<crate::agent::Badge>>,
    pointer_monitors: RefCell<Option<crate::platform::pointer::PointerMonitors>>,
}

struct AfterTick {
    sync: bool,
    menu: Option<MenuModel>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ShellIvars]
    pub struct ShellClock;

    impl ShellClock {
        #[unsafe(method(screensChanged:))]
        fn screens_changed(&self, _notification: Option<&NSNotification>) {
            self.ivars().screens_dirty.store(true, Ordering::Release);
            let running = self.ivars().state.borrow().power.sources().clock;
            if running {
                self.stop_timer();
                self.start_timer();
            }
        }

        #[unsafe(method(appActivated:))]
        fn app_activated(&self, _notification: Option<&NSNotification>) {
            self.ivars().app_activated.store(true, Ordering::Release);
            crate::agent::note_frontmost();
        }

        #[unsafe(method(agentBadge:))]
        fn agent_badge(&self, _sender: &AnyObject) {
            let host = self.ivars().state.borrow().agent.host.clone();
            crate::agent::focus::activate(host.as_deref());
        }

        #[unsafe(method(becameActive:))]
        fn became_active(&self, _notification: Option<&NSNotification>) {
            self.ivars().self_activated.store(true, Ordering::Release);
        }

        #[unsafe(method(screensSlept:))]
        fn screens_slept(&self, _: Option<&NSNotification>) {
            self.flag(|power| power.set_screens_asleep(true));
        }

        #[unsafe(method(screensWoke:))]
        fn screens_woke(&self, _: Option<&NSNotification>) {
            self.flag(|power| power.set_screens_asleep(false));
        }

        #[unsafe(method(willSleep:))]
        fn will_sleep(&self, _: Option<&NSNotification>) {
            self.flag(|power| power.set_system_sleep(true));
        }

        #[unsafe(method(didWake:))]
        fn did_wake(&self, _: Option<&NSNotification>) {
            self.flag(|power| power.set_system_sleep(false));
        }

        #[unsafe(method(sessionResigned:))]
        fn session_resigned(&self, _: Option<&NSNotification>) {
            self.flag(|power| power.set_session_inactive(true));
        }

        #[unsafe(method(sessionActive:))]
        fn session_active(&self, _: Option<&NSNotification>) {
            self.flag(|power| power.set_session_inactive(false));
        }

        #[unsafe(method(reduceMotion:))]
        fn reduce_motion(&self, _: Option<&NSNotification>) {
            let on = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
            self.flag(|power| power.set_reduce_motion(on));
        }

        #[unsafe(method(occlusion:))]
        fn occlusion(&self, _: Option<&NSNotification>) {
            if !self.ivars().state.borrow().occlusion_trusted {
                return;
            }
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let visible = NSApplication::sharedApplication(mtm)
                .occlusionState()
                .contains(NSApplicationOcclusionState::Visible);
            self.flag(|power| power.set_occluded(!visible));
        }

        #[unsafe(method(screenLocked:))]
        fn screen_locked(&self, _: Option<&NSNotification>) {
            self.flag(|power| power.set_locked(true));
        }

        #[unsafe(method(screenUnlocked:))]
        fn screen_unlocked(&self, _: Option<&NSNotification>) {
            self.flag(|power| power.set_locked(false));
        }

        #[unsafe(method(menuAct:))]
        fn menu_act(&self, sender: &AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            self.handle_menu(tag);
        }

        #[unsafe(method(statusAct:))]
        fn status_act(&self, sender: &AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            crate::ui::actions::status_act(self, tag);
        }

        #[unsafe(method(settingsPage:))]
        fn settings_page(&self,sender:&AnyObject) {
            let index:isize=unsafe {msg_send![sender,tag]};
            if let Some(ui)=self.ivars().settings_ui.borrow().as_ref() {
                if index>=0 {ui.select_page(index as usize);}
            }
        }

        #[unsafe(method(settingsAct:))]
        fn settings_act(&self, sender: &AnyObject) {
            crate::ui::actions::settings_act(self, sender);
        }

        #[unsafe(method(loginAct:))]
        fn login_act(&self, sender: &AnyObject) {
            crate::ui::actions::login_act(sender);
        }

        #[unsafe(method(mischiefAct:))]
        fn mischief_act(&self, sender: &AnyObject) {
            crate::ui::actions::mischief_act(self, sender);
        }

        #[unsafe(method(mischiefPromptAct:))]
        fn mischief_prompt_act(&self, sender: &AnyObject) {
            crate::ui::actions::mischief_prompt_act(self, sender);
        }
    }

    unsafe impl NSObjectProtocol for ShellClock {}

    unsafe impl NSMenuDelegate for ShellClock {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            crate::ui::status_menu::rebuild(self, menu);
        }
    }
);

impl ShellClock {
    pub fn new(mtm: MainThreadMarker, state: AppState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ShellIvars {
            state: RefCell::new(state),
            screens_dirty: AtomicBool::new(false),
            app_activated: AtomicBool::new(false),
            self_activated: AtomicBool::new(false),
            timer: RefCell::new(None),
            timer_wakeups: Cell::new(0),
            late_ticks: Cell::new(0),
            last_callback: Cell::new(None),
            menu_open: Cell::new(false),
            motion_trace: std::env::var_os("SHIMEJI_TRACE_MOTION").is_some(),
            status_item: RefCell::new(None),
            status_menu: RefCell::new(None),
            delegate: RefCell::new(None),
            settings_ui: RefCell::new(None),
            popover: RefCell::new(None),
            diagnostics_alert: RefCell::new(None),
            mischief_prompt: RefCell::new(None),
            badge: RefCell::new(None),
            pointer_monitors: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let ptr = std::ptr::from_ref(&*this) as usize;
        let monitors = crate::platform::pointer::PointerMonitors::new(move || {
            // Tokens are owned by this clock and removed when its ivars drop.
            let clock = unsafe { &*(ptr as *const ShellClock) };
            clock.refresh_pointer();
        });
        *this.ivars().pointer_monitors.borrow_mut() = Some(monitors);
        this
    }

    fn refresh_pointer(&self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Ok(mut state) = self.ivars().state.try_borrow_mut() else {
            return;
        };
        let _ = mtm;
        let loc = NSEvent::mouseLocation();
        let cursor = state
            .renderer
            .pointer_from_cocoa(loc.x.round() as i32, loc.y.round() as i32);
        state.renderer.refresh_mouse(cursor);
        for extra in &mut state.extras {
            extra.renderer.refresh_mouse(cursor);
        }
    }

    pub fn start_timer(&self) {
        let ptr = std::ptr::from_ref(self).cast::<AnyObject>();
        *self.ivars().timer.borrow_mut() = Some(GcdTimer::start(ptr, gcd_tick));
    }

    pub fn stop_timer(&self) {
        *self.ivars().timer.borrow_mut() = None;
    }

    pub fn install_hotkey(&self, chord: Chord) -> i32 {
        let user = std::ptr::from_ref(self).cast_mut().cast();
        match hotkey::Registration::install(chord, hotkey_fired, user) {
            Ok(reg) => {
                self.ivars().state.borrow_mut().hotkey = Some(reg);
                0
            }
            Err(code) => code,
        }
    }

    pub fn bind_input_wake(&self) {
        let ptr = std::ptr::from_ref(self) as usize;
        let queue = self.ivars().state.borrow().renderer.input().clone();
        queue.set_wake(Arc::new(move || {
            let clock = unsafe { &*(ptr as *const ShellClock) };
            clock.on_input();
        }));
    }

    pub fn observe(&self) {
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let local = NSNotificationCenter::defaultCenter();
        let distributed = NSDistributedNotificationCenter::defaultCenter();
        let locked = NSString::from_str("com.apple.screenIsLocked");
        let unlocked = NSString::from_str("com.apple.screenIsUnlocked");
        unsafe {
            workspace.addObserver_selector_name_object(
                self,
                sel!(screensSlept:),
                Some(NSWorkspaceScreensDidSleepNotification),
                None,
            );
            workspace.addObserver_selector_name_object(
                self,
                sel!(screensWoke:),
                Some(NSWorkspaceScreensDidWakeNotification),
                None,
            );
            workspace.addObserver_selector_name_object(
                self,
                sel!(willSleep:),
                Some(NSWorkspaceWillSleepNotification),
                None,
            );
            workspace.addObserver_selector_name_object(
                self,
                sel!(didWake:),
                Some(NSWorkspaceDidWakeNotification),
                None,
            );
            workspace.addObserver_selector_name_object(
                self,
                sel!(sessionResigned:),
                Some(NSWorkspaceSessionDidResignActiveNotification),
                None,
            );
            workspace.addObserver_selector_name_object(
                self,
                sel!(sessionActive:),
                Some(NSWorkspaceSessionDidBecomeActiveNotification),
                None,
            );
            workspace.addObserver_selector_name_object(
                self,
                sel!(reduceMotion:),
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
            );
            workspace.addObserver_selector_name_object(
                self,
                sel!(appActivated:),
                Some(NSWorkspaceDidActivateApplicationNotification),
                None,
            );
            local.addObserver_selector_name_object(
                self,
                sel!(screensChanged:),
                Some(NSApplicationDidChangeScreenParametersNotification),
                None,
            );
            local.addObserver_selector_name_object(
                self,
                sel!(occlusion:),
                Some(NSApplicationDidChangeOcclusionStateNotification),
                None,
            );
            local.addObserver_selector_name_object(
                self,
                sel!(becameActive:),
                Some(NSApplicationDidBecomeActiveNotification),
                None,
            );
            distributed.addObserver_selector_name_object(
                self,
                sel!(screenLocked:),
                Some(&*locked),
                None,
            );
            distributed.addObserver_selector_name_object(
                self,
                sel!(screenUnlocked:),
                Some(&*unlocked),
                None,
            );
        }
    }

    /// Apply the power controller to the clock, the poller, and visibility.
    pub fn sync_sources(&self) {
        let sources = self.ivars().state.borrow().power.sources();
        let timer_on = self.ivars().timer.borrow().is_some();
        if sources.clock && !timer_on {
            self.start_timer();
        } else if !sources.clock && timer_on {
            self.stop_timer();
        }
        let mut state = self.ivars().state.borrow_mut();
        let frozen = state.power.engine_frozen();
        let visible = state.power.pets_visible();
        let pace_now = state.poller.ctrl.pace.load(Ordering::Acquire);
        if sources.poller {
            if pace_now == PollPace::Stopped as u32 {
                state.poller.ctrl.set_pace(PollPace::Far);
            }
        } else if pace_now != PollPace::Stopped as u32 {
            state.poller.ctrl.set_pace(PollPace::Stopped);
        }
        if state.runtime.paused() != frozen {
            state.runtime.set_paused(frozen);
        }
        state.renderer.input().set_deliver_now(!sources.clock);
        if visible {
            if !state.pets_shown {
                state.renderer.show_all();
                state.pets_shown = true;
            }
        } else if state.pets_shown {
            state.occlusion_trusted = false;
            state.power.set_occluded(false);
            state.renderer.hide_all();
            state.pets_shown = false;
        }
        crate::ui::actions::sync_extras(&mut state);
    }

    pub fn on_input(&self) {
        let after = self.pump(false);
        self.finish(after);
    }

    pub fn on_tick(&self) {
        self.note_timer();
        let after = self.pump(true);
        self.finish(after);
    }

    /// One frame at t≈0, used to stand a pet on the floor before the timer.
    pub fn shutdown_mischief(&self) {
        self.ivars().state.borrow_mut().mischief.shutdown();
    }

    pub fn present_now(&self) {
        self.on_tick();
    }

    pub fn settled(&self) -> bool {
        self.ivars().state.borrow().settled
    }

    pub fn timer_wakeups(&self) -> u64 {
        self.ivars().timer_wakeups.get()
    }

    /// Wakeups after the S5 pause began. The settling tick is the baseline.
    pub fn timer_delta(&self) -> u64 {
        let total = self.timer_wakeups();
        match self.ivars().state.borrow().measure_base {
            Some(base) => total.saturating_sub(base),
            None => total,
        }
    }

    pub fn snapshot_counters(&self) -> Totals {
        let state = self.ivars().state.borrow();
        let commits = state.renderer.counters;
        let (queries, query_us) = state.poller.ctrl.query_totals();
        let mut totals = Totals {
            callbacks: self.ivars().timer_wakeups.get(),
            ticks: state.ticks,
            late_ticks: self.ivars().late_ticks.get(),
            resyncs: state.resyncs,
            window_moves: commits.window_moves,
            layer_moves: commits.layer_moves,
            contents_sets: commits.contents_sets,
            window_queries: queries,
            window_query_us: query_us,
            mouse_events: state.mouse_events,
            timer_wakeups: self.ivars().timer_wakeups.get(),
            decoded_image_bytes: state.renderer.decoded_bytes(),
        };
        crate::ui::actions::sum_extra_counters(&state, &mut totals);
        totals
    }

    pub fn sound_stats(&self) -> (bool, u64) {
        match self.ivars().state.borrow().player.as_ref() {
            Some(player) => (player.started(), player.scheduled()),
            None => (false, 0),
        }
    }

    fn flag(&self, apply: impl FnOnce(&mut Power)) {
        {
            let mut state = self.ivars().state.borrow_mut();
            apply(&mut state.power);
        }
        self.sync_sources();
    }

    fn toggle_hidden(&self) {
        let Ok(mut state) = self.ivars().state.try_borrow_mut() else {
            return;
        };
        state.power.toggle_hidden();
        drop(state);
        self.sync_sources();
    }

    fn note_timer(&self) {
        let ivars = self.ivars();
        let now = Instant::now();
        if let Some(prev) = ivars.last_callback.get() {
            let gap = u64::try_from(prev.elapsed().as_millis()).unwrap_or(u64::MAX);
            if counters::gap_is_late(gap, STEP_MS) {
                ivars.late_ticks.set(ivars.late_ticks.get() + 1);
            }
        }
        ivars.last_callback.set(Some(now));
        ivars.timer_wakeups.set(ivars.timer_wakeups.get() + 1);
    }

    fn pump(&self, from_timer: bool) -> AfterTick {
        let ivars = self.ivars();
        let Ok(mut state) = ivars.state.try_borrow_mut() else {
            return AfterTick {
                sync: false,
                menu: None,
            };
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return AfterTick {
                sync: false,
                menu: None,
            };
        };
        crate::agent::flush_pending(&mut state);
        if ivars.screens_dirty.swap(false, Ordering::AcqRel) {
            refresh_screens(&mut state, mtm);
        }
        if ivars.app_activated.swap(false, Ordering::AcqRel) {
            if let Some(pid) = frontmost_pid() {
                if pid != state.our_pid {
                    state
                        .poller
                        .ctrl
                        .frontmost_pid
                        .store(pid, Ordering::Release);
                }
            }
            state.poller.ctrl.need_full.store(true, Ordering::Release);
        }
        if ivars.self_activated.swap(false, Ordering::AcqRel) {
            state.mischief.on_activate();
        }
        let trust_poll = state.power.sources().ax_trust_poll;
        state.mischief.on_clock(trust_poll);

        let space = read_space(mtm);
        let loc = NSEvent::mouseLocation();
        let cursor = space.point_to_engine(loc.x.round() as i32, loc.y.round() as i32);
        let frozen = state.power.engine_frozen();
        let allow_menu = !ivars.menu_open.get() && !state.power.hidden();
        let mut pending_menu: Option<MascotId> = None;
        let events = state.renderer.input().drain();
        for event in events {
            state.mouse_events += 1;
            if ivars.motion_trace {
                eprintln!(
                    "motion_input ms={:.3} event={event:?} cursor={},{} frozen={frozen}",
                    state.origin.elapsed().as_secs_f64() * 1000.0,
                    cursor.x,
                    cursor.y,
                );
            }
            match event {
                InputEvent::LeftDown(id) if frozen => {
                    if let Some(frame) = state
                        .runtime
                        .frames()
                        .into_iter()
                        .find(|item| item.id == id)
                    {
                        state.drag = Some(FrozenDrag {
                            id,
                            dx: frame.anchor.x - cursor.x,
                            dy: frame.anchor.y - cursor.y,
                        });
                    }
                }
                InputEvent::LeftDown(id) => {
                    state.drag = None;
                    state.runtime.push(Command::Press {
                        mascot: Some(id),
                        button: MouseButton::Left,
                    });
                }
                InputEvent::LeftUp(id) if frozen => {
                    if state.drag.as_ref().is_some_and(|drag| drag.id == id) {
                        state.drag = None;
                    }
                }
                InputEvent::LeftUp(id) => {
                    state.runtime.push(Command::Release {
                        mascot: Some(id),
                        button: MouseButton::Left,
                    });
                }
                InputEvent::Drag(id) if frozen => {
                    let offset = state
                        .drag
                        .filter(|drag| drag.id == id)
                        .map(|drag| (drag.dx, drag.dy));
                    if let Some((dx, dy)) = offset {
                        state.runtime.push(Command::MoveFrozen {
                            id,
                            anchor: Point::new(cursor.x + dx, cursor.y + dy),
                        });
                    }
                }
                InputEvent::Drag(_) => {}
                InputEvent::RightDown(id) => {
                    if allow_menu {
                        pending_menu = Some(id);
                    }
                }
            }
        }

        let snapshot = Snapshot {
            screens: space.snapshot_screens(),
            cursor,
            active_window: state.poller.slot.load_sample(),
            multiscreen: state.settings.multiscreen,
        };
        if state.mischief.phase() != Phase::Off {
            publish_target(&state, &snapshot);
        }
        let events = state.mischief.drain();
        for event in events {
            state.runtime.push(Command::Window(event));
            for extra in &mut state.extras {
                extra.runtime.push(Command::Window(event));
            }
        }
        let now_ms = state.origin.elapsed().as_secs_f64() * 1000.0;
        let (ticks, resynced, sounds, spawned, merged, moved) = {
            let frame = state.runtime.on_frame(now_ms, &snapshot);
            let spawned: Vec<MascotId> = frame.spawned.to_vec();
            let sounds: Vec<(String, Option<String>)> = frame
                .sounds
                .iter()
                .map(|event| (event.name.clone(), event.volume.clone()))
                .collect();
            let mut merged: Vec<MascotDelta> = Vec::new();
            for tick in frame.deltas {
                for delta in &tick.mascots {
                    merge_delta(&mut merged, *delta);
                }
            }
            for delta in frame.frozen {
                merge_delta(&mut merged, *delta);
            }
            let moved = if frame.ticks > 0 {
                frame.window_moves.iter().map(|item| item.id).collect()
            } else {
                Vec::new()
            };
            (frame.ticks, frame.resynced, sounds, spawned, merged, moved)
        };
        if from_timer {
            state.ticks += u64::from(ticks);
            if resynced {
                state.resyncs += 1;
            }
        }
        play_sounds(&mut state, &sounds);
        crate::ui::actions::restore_primary(&mut state, &spawned);
        let live = state.runtime.frames();
        let ready = take_ready(&mut state.awaiting_windows, &spawned, ticks > 0, |id| {
            live.iter().any(|item| item.id == id)
        });
        let mut announce = !state.announced_first_x && state.renderer.live_ids().is_empty();
        for id in ready {
            if let Some(mascot) = live.iter().find(|item| item.id == id) {
                if announce {
                    println!("first_x={}", mascot.anchor.x);
                    announce = false;
                    state.announced_first_x = true;
                }
                state.renderer.attach(id, mascot);
            }
        }
        let attached = state.renderer.live_ids();
        for id in attached {
            if live.iter().all(|item| item.id != id) {
                state.renderer.detach(id);
            }
        }
        state.renderer.commit(&live, &merged);
        if ivars.motion_trace {
            eprintln!(
                "motion_environment ms={now_ms:.3} cached_pid={} actual_pid={:?} window={:?} screens={:?}",
                state.poller.ctrl.frontmost_pid.load(Ordering::Acquire),
                frontmost_pid(),
                snapshot.active_window,
                snapshot.screens
            );
            for frame in &live {
                eprintln!(
                    "motion_frame ms={now_ms:.3} ticks={ticks} resynced={resynced} cursor={},{} frame={frame:?} drawn_cocoa={:?} geometry={:?}",
                    cursor.x,
                    cursor.y,
                    state.renderer.sprite_cocoa(frame.id),
                    state.renderer.trace_geometry(frame.id),
                );
            }
        }

        if !state.occlusion_trusted
            && state.power.pets_visible()
            && !state.renderer.live_ids().is_empty()
        {
            let app_visible = NSApplication::sharedApplication(mtm)
                .occlusionState()
                .contains(NSApplicationOcclusionState::Visible);
            if app_visible {
                state.occlusion_trusted = true;
            }
        }

        if let Some(walk) = state.walk.clone() {
            if !state.power.engine_frozen() {
                let ids: Vec<MascotId> = state
                    .runtime
                    .frames()
                    .into_iter()
                    .filter(|frame| {
                        frame.behavior != walk
                            && frame.behavior != FALL_JA
                            && frame.behavior != FALL_EN
                    })
                    .map(|frame| frame.id)
                    .collect();
                for id in ids {
                    state.runtime.push(Command::SetBehavior {
                        id: Some(id),
                        name: walk.clone(),
                    });
                }
            }
        }

        let mut sync = false;
        if state.power.arming()
            && state.spawn_target > 0
            && state.runtime.population() >= state.spawn_target
        {
            state.power.set_arming(false);
            if state.settle_pause && !state.settled {
                state.power.set_user_paused(true);
                state.settled = true;
                state.measure_base = Some(ivars.timer_wakeups.get());
            }
            sync = true;
        }

        let extra = crate::ui::actions::tick_extras(
            &mut state, &space, cursor, now_ms, from_timer, allow_menu,
        );
        let running = state.power.sources().poller;
        let pace = state
            .poll_actions
            .pace(running, &live, snapshot.active_window.as_ref());
        state.poller.ctrl.set_pace(if running {
            pace.merge(extra.poll_pace)
        } else {
            PollPace::Stopped
        });
        state.renderer.refresh_mouse(cursor);
        for slot in &mut state.extras {
            slot.renderer.refresh_mouse(cursor);
        }
        if state.mischief.worker_running() && (ticks > 0 || extra.ticks > 0) {
            let mut active = state
                .poll_actions
                .carry_windows(&live, snapshot.active_window.as_ref());
            active.extend(moved);
            active.extend(extra.carrying);
            if extra.ticks > 0 {
                active.extend(extra.moved);
            }
            active.sort_unstable();
            active.dedup();
            state.mischief.note_activity(&active);
        }
        let (mascot, toggles) = if let Some((index, id)) = extra.menu {
            state.menu_extra = Some(index);
            let toggles = state
                .extras
                .get(index)
                .map(|extra| extra.toggles.clone())
                .unwrap_or_default();
            (Some(id), toggles)
        } else {
            state.menu_extra = None;
            (pending_menu, state.toggles.clone())
        };
        state.menu_mascot = mascot;
        let menu = mascot.map(|_| MenuModel {
            user_paused: state.power.user_paused(),
            toggles,
        });
        crate::agent::note_pose(&mut state);
        AfterTick { sync, menu }
    }

    fn finish(&self, after: AfterTick) {
        if after.sync {
            self.sync_sources();
        }
        if let Some(model) = after.menu {
            if !self.ivars().menu_open.get() {
                if let Some(mtm) = MainThreadMarker::new() {
                    self.ivars().menu_open.set(true);
                    let target = std::ptr::from_ref(self).cast::<AnyObject>();
                    context_menu::popup(mtm, &model, unsafe { &*target });
                    self.ivars().menu_open.set(false);
                }
            }
        }
        crate::ui::actions::sync_mischief_chrome(self);
        crate::agent::place_badge(self);
    }

    fn handle_menu(&self, tag: isize) {
        crate::ui::actions::context_menu(self, tag);
        self.sync_sources();
    }
}

fn publish_target(state: &AppState, snapshot: &Snapshot) {
    let (window, bounds) = match snapshot.active_window {
        Some(window) => (
            window.id,
            Bounds {
                x: window.x,
                y: window.y,
                width: window.width,
                height: window.height,
            },
        ),
        None => (
            0,
            Bounds {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
        ),
    };
    let screens = snapshot
        .screens
        .iter()
        .map(|screen| Bounds {
            x: screen.x,
            y: screen.y,
            width: screen.width,
            height: screen.height,
        })
        .collect();
    let pid = state.poller.ctrl.frontmost_pid.load(Ordering::Acquire);
    state.mischief.publish(
        pid,
        window,
        bounds,
        screens,
        state.settings.window_rules.clone(),
    );
}

fn merge_delta(merged: &mut Vec<MascotDelta>, delta: MascotDelta) {
    if let Some(existing) = merged.iter_mut().find(|item| item.id == delta.id) {
        existing.anchor_changed |= delta.anchor_changed;
        existing.image_changed |= delta.image_changed;
    } else {
        merged.push(delta);
    }
}

fn play_sounds(state: &mut AppState, sounds: &[(String, Option<String>)]) {
    if !state.mute.allows() || sounds.is_empty() {
        return;
    }
    let frames: Vec<_> = sounds
        .iter()
        .filter_map(|(name, volume)| {
            let clip = state.library.get(name)?;
            Some(sound::to_float_frames(
                clip,
                sound::gain_from_volume(volume.as_deref()),
            ))
        })
        .collect();
    if let Some(player) = state.player.as_mut() {
        for frame in &frames {
            player.play_frames(frame);
        }
    }
}

fn refresh_screens(state: &mut AppState, mtm: MainThreadMarker) {
    let space = read_space(mtm);
    let rects = engine_screen_rects(&space);
    state.renderer.set_space(space.clone());
    crate::ui::actions::refresh_extra_screens(state, &space);
    state.poller.set_screens(rects);
    state.main_display = main_display_id(mtm);
}

unsafe extern "C" fn gcd_tick(context: *mut c_void) {
    if context.is_null() {
        return;
    }
    let clock = unsafe { &*context.cast::<ShellClock>() };
    clock.on_tick();
}

unsafe extern "C" fn hotkey_fired(user: *mut c_void) {
    let ptr = user as usize;
    crate::threads::wake::on_main(move || {
        if ptr == 0 {
            return;
        }
        let clock = unsafe { &*(ptr as *const ShellClock) };
        clock.toggle_hidden();
    });
}

fn frontmost_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

/// Keep the shared application name so dependents stay linked.
pub fn finish_launch(mtm: MainThreadMarker) -> Retained<NSApplication> {
    let app = NSApplication::sharedApplication(mtm);
    app.finishLaunching();
    app
}
