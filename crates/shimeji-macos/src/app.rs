//! Accessory app: load the bundled shime, spawn pets, run the clock.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Instant;

use objc2::{DefinedClass, MainThreadMarker, rc::autoreleasepool};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEventMask, NSWorkspace};
use objc2_foundation::{NSBundle, NSDate, NSDefaultRunLoopMode};
use shimeji_app::first_run;
use shimeji_app::import::{self, Choice, Outcome};
use shimeji_app::journal::Bounds;
use shimeji_app::library::Library as PackLibrary;
use shimeji_app::settings::{self, EnvOverrides};
use shimeji_app::{Command, Runtime};
use shimeji_core::env::{Point, WindowSample};
use shimeji_core::{JavaRandom, WindowControl};

use crate::bridge::{AppState, ShellClock, finish_launch};
use crate::launch;
use crate::mischief;
use crate::platform::coords::CocoaSpace;
use crate::platform::counters::{self, Totals};
use crate::platform::hotkey::{self, Chord};
use crate::platform::image_cache::ImageCache;
use crate::platform::input::InputQueue;
use crate::platform::panel::PanelRenderer;
use crate::platform::poses::PoseTable;
use crate::platform::power::{self, Power};
use crate::platform::screens::{engine_screen_rects, main_display_id, read_space};
use crate::platform::slack::DEFAULT_SLACK;
use crate::platform::sound::{self, Library, Mute};
use crate::platform::spawn::{spawn_anchor, visible_drop_anchor};
use crate::platform::window_env::{full_list, pick, to_window_sample};
use crate::threads::poller::Poller;
use crate::ui::actions::{self, PackCache};
use crate::ui::first_run::{main_floor, primary_work};
use crate::ui::{install_shell, show_popover};

const WALK: &str = "ワークエリアの下辺を歩く";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    /// Part A default: one pet on the seeded behavior mix.
    Run,
    Baseline,
    S1,
    S2,
    S3,
    S4,
    S5,
}

impl Scenario {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "run" => Self::Run,
            "baseline" => Self::Baseline,
            "s1" => Self::S1,
            "s2" => Self::S2,
            "s3" => Self::S3,
            "s4" => Self::S4,
            "s5" => Self::S5,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Baseline => "baseline",
            Self::S1 => "s1",
            Self::S2 => "s2",
            Self::S3 => "s3",
            Self::S4 => "s4",
            Self::S5 => "s5",
        }
    }

    fn pets(self) -> usize {
        match self {
            Self::Baseline => 0,
            Self::Run | Self::S1 | Self::S2 | Self::S5 => 1,
            Self::S3 => 10,
            Self::S4 => 50,
        }
    }

    fn walk(self) -> Option<String> {
        match self {
            Self::S1 => Some(WALK.to_string()),
            _ => None,
        }
    }

    fn settle_pause(self) -> bool {
        matches!(self, Self::S5)
    }
}

struct Args {
    seconds: Option<f64>,
    /// `None` when `--seed` was not passed.
    seed: Option<i64>,
    slack: u32,
    scenario: Scenario,
    profile: Option<PathBuf>,
    /// Socket home for agent reactions. Absent means the passwd home.
    agent_home: Option<PathBuf>,
}

impl Args {
    fn parse() -> Self {
        let mut seconds = None;
        let mut seed = None;
        let mut slack = DEFAULT_SLACK;
        let mut scenario = Scenario::Run;
        let mut profile = None;
        let mut agent_home = None;
        let mut it = std::env::args().skip(1);
        while let Some(key) = it.next() {
            match key.as_str() {
                "--help" | "-h" => {
                    println!(
                        "shimeji-macos --seconds N --seed N --slack 1|2|4|8 --scenario run|baseline|s1|s2|s3|s4|s5 --profile DIR --agent-home DIR"
                    );
                    std::process::exit(0);
                }
                "--seconds" => seconds = Some(parse_num(&need(&mut it, &key), &key)),
                "--seed" => seed = Some(parse_num(&need(&mut it, &key), &key)),
                "--slack" => slack = parse_num(&need(&mut it, &key), &key),
                "--profile" => profile = Some(PathBuf::from(need(&mut it, &key))),
                "--agent-home" => agent_home = Some(PathBuf::from(need(&mut it, &key))),
                "--scenario" => {
                    let value = need(&mut it, &key);
                    scenario = Scenario::parse(&value).unwrap_or_else(|| {
                        eprintln!("--scenario must be run, baseline, s1, s2, s3, s4, or s5");
                        std::process::exit(2);
                    });
                }
                other => {
                    eprintln!("unknown argument {other}");
                    std::process::exit(2);
                }
            }
        }
        let seconds = launch::launch_seconds(seconds, scenario.name()).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2);
        });
        if !matches!(slack, 1 | 2 | 4 | 8) {
            eprintln!("--slack must be 1, 2, 4 or 8");
            std::process::exit(2);
        }
        Self {
            seconds,
            seed,
            slack,
            scenario,
            profile,
            agent_home,
        }
    }
}

fn need(it: &mut impl Iterator<Item = String>, key: &str) -> String {
    it.next().unwrap_or_else(|| {
        eprintln!("missing value for {key}");
        std::process::exit(2);
    })
}

fn parse_num<T: std::str::FromStr>(value: &str, key: &str) -> T {
    value.parse().unwrap_or_else(|_| {
        eprintln!("bad number for {key}: {value}");
        std::process::exit(2);
    })
}

pub fn run() -> u8 {
    let args = Args::parse();
    let seed = launch::launch_seed(args.seed, args.scenario.name(), launch::clock_nanos());
    println!("seed={seed}");
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("not on the main thread");
        return 1;
    };
    let app = finish_launch(mtm);
    let accessory = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    println!("activation_policy_accessory={accessory}");
    println!("scenario={}", args.scenario.name());

    let source = bundled_shime();
    let packs_root = args.profile.clone().unwrap_or_else(app_support);
    println!("profile={}", packs_root.display());
    println!("pack_source={}", source.display());
    println!("packs_root={}", packs_root.display());
    println!("load=import");
    if let Err(error) = std::fs::create_dir_all(&packs_root) {
        eprintln!("create {}: {error}", packs_root.display());
        return 1;
    }
    let loaded = settings::load(&packs_root);
    if let Some(backup) = &loaded.backup {
        println!("settings_backup={}", backup.display());
    }
    let mut settings = loaded.settings;
    let animate_env = std::env::var(power::ANIMATE_ANYWAY_ENV).ok();
    let sound_env = std::env::var(sound::SOUND_ENV).ok();
    let hotkey_env = std::env::var(hotkey::HOTKEY_ENV).ok();
    let mischief_env = std::env::var("SHIMEJI_MISCHIEF").ok();
    settings.apply_env(EnvOverrides {
        animate_anyway: animate_env.as_deref(),
        sound: sound_env.as_deref(),
        hotkey: hotkey_env.as_deref(),
        mischief: mischief_env.as_deref(),
    });
    let outcome = match import::import(&source, &packs_root, Some(Choice::Replace)) {
        Outcome::Success(installed) | Outcome::Degraded(installed) => installed,
        Outcome::Error { message } => {
            eprintln!("import failed: {message}");
            return 1;
        }
        Outcome::Duplicate { id } => {
            eprintln!("import failed: duplicate {id}");
            return 1;
        }
    };
    println!("import_id={}", outcome.id);
    println!("import_path={}", outcome.path.display());
    let Some(character) = outcome.pack.characters.first().cloned() else {
        eprintln!("imported pack has no characters");
        return 1;
    };
    println!("character={}", character.name);
    println!("decoded_images={}", outcome.images.len());

    let mut catalog = PackLibrary::open(&packs_root);
    catalog.insert(
        &outcome.id,
        &outcome.path,
        &outcome.pack,
        &outcome.diagnostics,
    );

    let chase = character
        .behavior_schema
        .required_behaviors()
        .first()
        .copied()
        .unwrap_or("ChaseMouse")
        .to_string();
    println!("chase={chase}");

    let mut library = Library::default();
    for wav in &outcome.sounds {
        library.insert(
            &wav.path,
            sound::Clip {
                channels: wav.channels,
                sample_rate: wav.sample_rate,
                bits_per_sample: wav.bits_per_sample,
                pcm: wav.pcm.clone(),
            },
        );
    }
    let sound_on = settings.sounds;
    let debug_tone = sound::env_flag(std::env::var(sound::DEBUG_TONE_ENV).ok().as_deref());
    let mute = Mute { enabled: sound_on };
    let mut player = if sound_on || debug_tone {
        Some(sound::Player::new())
    } else {
        None
    };
    if debug_tone {
        if let Some(engine) = player.as_mut() {
            let frames = sound::to_float_frames(&sound::debug_tone(), 1.0);
            let played = engine.play_frames(&frames);
            println!("debug_tone={played}");
        }
    }
    println!("sound={}", if sound_on { "on" } else { "muted" });

    let space = read_space(mtm);
    println!("primary_height={}", space.primary_height);
    println!("screen_count={}", space.screens.len());
    if let Some(work) = primary_work(&space) {
        println!(
            "primary_work={},{},{},{}",
            work.x, work.y, work.width, work.height
        );
    }
    let poses = PoseTable::new(&character, &outcome.images);
    let cache = ImageCache::from_import(&character, &outcome.images);
    let queue = InputQueue::default();
    let mut renderer = PanelRenderer::new(mtm, cache, poses, space.clone(), queue, args.slack);
    let session = mischief::Session::boot(
        packs_root.clone(),
        settings.mischief,
        settings.window_rules.clone(),
        screen_bounds(&space),
    );
    let mut runtime = Runtime::with_windows(&character, seed, session.handle());
    let toggles = actions::prepare_primary(
        &mut runtime,
        &mut renderer,
        &settings,
        &outcome.id,
        &character,
    );
    println!("toggleable={}", toggles.len());

    let reduce_motion = NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
    let animate_anyway = settings.animate_anyway;
    let launch = first_run::plan(first_run::load(&packs_root), reduce_motion, animate_anyway);
    let mut pets = args.scenario.pets();
    if args.scenario == Scenario::Run && !settings.character_enabled(&outcome.id, &character.name) {
        pets = 0;
    }
    let place = launch.place_on_floor && pets == 1;
    println!("place_on_floor={place}");
    let mut drop_rng = JavaRandom::new(seed);
    let mut pending_drops = VecDeque::new();
    let our_pid = std::process::id() as i32;
    let frontmost = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
        .unwrap_or(our_pid);
    // One launch sample avoids racing the async poller's first publication.
    // Steady-state queries remain owned by the adaptive poller.
    let initial_window = if !place && pets > 0 && settings.keep_on_screen {
        pick(
            &full_list(),
            frontmost,
            our_pid,
            &engine_screen_rects(&space),
        )
        .map(|active| to_window_sample(&active))
    } else {
        None
    };
    if !place {
        push_drop_spawns(
            &mut runtime,
            pets,
            &space,
            &mut drop_rng,
            &mut pending_drops,
            initial_window.as_ref(),
            renderer.drop_headroom(),
        );
    }

    let mut power = Power::default();
    power.set_reduce_motion(reduce_motion);
    power.set_animate_anyway(animate_anyway);
    if pets == 0 {
        // No-pet baseline: hold the pause flag so the clock and poller stay stopped.
        power.set_user_paused(true);
    } else {
        power.set_arming(true);
    }
    println!("reduce_motion={reduce_motion}");
    println!("animate_anyway={animate_anyway}");

    let poller = Poller::start(our_pid, frontmost, engine_screen_rects(&space));
    let agents_on = settings.agents;
    let socket_home = args
        .agent_home
        .clone()
        .or_else(|| shimeji_proto::home_dir().ok());
    let pack_id = outcome.id.clone();
    let character_name = character.name.clone();
    let caches = vec![PackCache {
        id: pack_id.clone(),
        pack: outcome.pack,
        images: outcome.images,
        sounds: outcome.sounds,
    }];

    let clock = ShellClock::new(
        mtm,
        AppState {
            runtime,
            renderer,
            poller,
            poll_actions: crate::platform::window_env::PollingActions::new(&character),
            our_pid,
            origin: Instant::now(),
            ticks: 0,
            resyncs: 0,
            mouse_events: 0,
            main_display: main_display_id(mtm),
            power,
            chase,
            toggles,
            menu_mascot: None,
            drag: None,
            library,
            mute,
            player,
            spawn_target: pets,
            walk: args.scenario.walk(),
            settle_pause: args.scenario.settle_pause(),
            settled: false,
            measure_base: None,
            pets_shown: false,
            occlusion_trusted: false,
            hotkey: None,
            support_dir: packs_root.clone(),
            settings,
            catalog,
            pack_id,
            character_name,
            extras: Vec::new(),
            menu_extra: None,
            caches,
            seed,
            drop_rng,
            announced_first_x: false,
            awaiting_windows: Vec::new(),
            pending_drops,
            drop_hold: false,
            slack: args.slack,
            mischief: session,
            agent: crate::agent::react::Session::new(socket_home),
        },
    );
    clock.observe();
    clock.bind_input_wake();
    let spec = clock.ivars().state.borrow().settings.hotkey.clone();
    let chord = hotkey_chord(&spec);
    let hotkey_status = clock.install_hotkey(chord);
    println!("hotkey={chord}");
    println!("hotkey_status={hotkey_status}");
    install_shell(&clock);
    clock.sync_sources();
    crate::agent::note_frontmost();
    println!("agent_hooks={}", crate::agent::hooks_word());
    if agents_on {
        crate::agent::start(&clock);
    } else {
        println!("agent_listener=off");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
    if place {
        if let Some(anchor) = main_floor(&read_space(mtm)) {
            println!("floor_anchor={},{}", anchor.x, anchor.y);
            clock
                .ivars()
                .state
                .borrow_mut()
                .runtime
                .push(Command::SpawnAt(anchor));
            clock.present_now();
        } else {
            let space = read_space(mtm);
            let anchor = {
                let mut state = clock.ivars().state.borrow_mut();
                let window = state.poller.slot.load_sample();
                let headroom = state.renderer.drop_headroom();
                next_drop(&space, &mut state.drop_rng, window.as_ref(), headroom)
            };
            let mut state = clock.ivars().state.borrow_mut();
            match anchor {
                Some(anchor) => {
                    let already = state.runtime.paused();
                    if !already && !state.drop_hold {
                        state.drop_hold = true;
                        state.runtime.set_paused(true);
                    }
                    state.pending_drops.push_back(anchor);
                    state.runtime.push(Command::SpawnAt(anchor));
                }
                None => state.runtime.push(Command::Spawn),
            }
        }
    }
    if args.scenario == Scenario::Run && launch.show_popover {
        pump(0.1);
        show_popover(&clock, launch.mention_animate_anyway, &chord.to_string());
    } else {
        println!("first_run_popover=skipped");
    }
    println!("gcd_interval_ms=40");
    println!("gcd_leeway_ms=1");
    println!("slack={}", args.slack);

    if args.scenario.settle_pause() {
        let warmup = Instant::now();
        while warmup.elapsed().as_secs_f64() < 5.0 && !clock.settled() {
            pump(0.05);
        }
        println!("settled={}", clock.settled());
        println!("warmup_s={:.3}", warmup.elapsed().as_secs_f64());
    }

    let cpu_start = cpu_secs();
    let ws_start = window_server_secs();
    let wakes_start = clock.timer_delta();
    let wall = Instant::now();
    pump_until(args.seconds);
    let elapsed = wall.elapsed().as_secs_f64();
    crate::agent::stop(&clock);
    clock.stop_timer();
    clock.shutdown_mischief();
    let cpu_end = cpu_secs();
    let ws_end = window_server_secs();
    let totals = clock.snapshot_counters();
    let (sound_started, sound_scheduled) = clock.sound_stats();
    print_report(ExitReport {
        scenario: args.scenario,
        elapsed,
        cpu_secs: cpu_end - cpu_start,
        ws_start,
        ws_end,
        totals,
        timer_delta: clock.timer_delta().saturating_sub(wakes_start),
        sound_started,
        sound_scheduled,
    });
    0
}

fn next_drop(
    space: &CocoaSpace,
    rng: &mut JavaRandom,
    window: Option<&WindowSample>,
    headroom: i32,
) -> Option<Point> {
    let work = primary_work(space)?;
    let anchor = visible_drop_anchor(spawn_anchor(work, rng), work, window, headroom);
    println!("drop_anchor={},{}", anchor.x, anchor.y);
    Some(anchor)
}

fn push_drop_spawns<W: WindowControl>(
    runtime: &mut Runtime<W>,
    count: usize,
    space: &CocoaSpace,
    rng: &mut JavaRandom,
    pending: &mut VecDeque<Point>,
    window: Option<&WindowSample>,
    headroom: i32,
) {
    for _ in 0..count {
        match next_drop(space, rng, window, headroom) {
            Some(anchor) => {
                pending.push_back(anchor);
                runtime.push(Command::SpawnAt(anchor));
            }
            None => runtime.push(Command::Spawn),
        }
    }
}

fn hotkey_chord(spec: &str) -> Chord {
    match hotkey::parse(spec) {
        Ok(chord) => chord,
        Err(error) => {
            eprintln!("hotkey_parse={error}");
            hotkey::default_chord()
        }
    }
}

struct ExitReport {
    scenario: Scenario,
    elapsed: f64,
    cpu_secs: f64,
    ws_start: Option<f64>,
    ws_end: Option<f64>,
    totals: Totals,
    timer_delta: u64,
    sound_started: bool,
    sound_scheduled: u64,
}

fn print_report(report: ExitReport) {
    let ExitReport {
        scenario,
        elapsed,
        cpu_secs,
        ws_start,
        ws_end,
        totals,
        timer_delta,
        sound_started,
        sound_scheduled,
    } = report;
    let rates = totals.rates(elapsed);
    let cpu_pct = if elapsed > 0.0 {
        cpu_secs / elapsed * 100.0
    } else {
        0.0
    };
    println!("wall_s={elapsed:.3}");
    println!("scenario={}", scenario.name());
    println!("rusage_cpu_percent={cpu_pct:.3}");
    match (ws_start, ws_end) {
        (Some(start), Some(end)) if elapsed > 0.0 => {
            let pct = (end - start).max(0.0) / elapsed * 100.0;
            println!("window_server_cpu_percent={pct:.3}");
        }
        _ => println!("window_server_cpu_percent=na"),
    }
    println!("callbacks={}", totals.callbacks);
    println!("ticks={}", totals.ticks);
    println!("callback_hz={:.3}", rates.callbacks_per_s);
    println!("tick_hz={:.3}", rates.ticks_per_s);
    println!("late_ticks={}", totals.late_ticks);
    println!("late_ticks_per_s={:.3}", rates.late_ticks_per_s);
    println!("resyncs={}", totals.resyncs);
    println!("resyncs_per_s={:.3}", rates.resyncs_per_s);
    println!("contents_sets={}", totals.contents_sets);
    println!("contents_sets_per_s={:.3}", rates.contents_sets_per_s);
    println!("layer_moves={}", totals.layer_moves);
    println!("layer_moves_per_s={:.3}", rates.layer_moves_per_s);
    println!("window_moves={}", totals.window_moves);
    println!("window_moves_per_s={:.3}", rates.window_moves_per_s);
    println!("window_queries={}", totals.window_queries);
    println!("window_queries_per_s={:.3}", rates.window_queries_per_s);
    println!("query_us={:.1}", rates.query_us);
    println!("mouse_events={}", totals.mouse_events);
    println!("mouse_events_per_s={:.3}", rates.mouse_events_per_s);
    println!("timer_wakeups={}", totals.timer_wakeups);
    println!("timer_wakeups_per_s={:.3}", rates.timer_wakeups_per_s);
    println!("timer_wakeups_delta={timer_delta}");
    println!("decoded_image_mb={:.3}", rates.decoded_image_mb);
    println!("sound_started={sound_started}");
    println!("sound_scheduled={sound_scheduled}");
}

fn bundled_shime() -> PathBuf {
    #[cfg(debug_assertions)]
    let fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime");
    #[cfg(not(debug_assertions))]
    let fallback = PathBuf::from("assets/packs/shime");
    crate::bundle_pack::bundled_pack_dir(main_bundle_resources().as_deref(), &fallback)
}

fn main_bundle_resources() -> Option<PathBuf> {
    let path = NSBundle::mainBundle().resourcePath()?;
    Some(PathBuf::from(crate::ui::lossy(&path)))
}

fn screen_bounds(space: &crate::platform::coords::CocoaSpace) -> Vec<Bounds> {
    space
        .snapshot_screens()
        .into_iter()
        .map(|screen| Bounds {
            x: screen.x,
            y: screen.y,
            width: screen.width,
            height: screen.height,
        })
        .collect()
}

fn app_support() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join("Library/Application Support/shimeji2")
}

fn pump(seconds: f64) {
    pump_until(Some(seconds));
}

fn pump_until(seconds: Option<f64>) {
    let end = seconds.map_or_else(NSDate::distantFuture, NSDate::dateWithTimeIntervalSinceNow);
    let app = NSApplication::sharedApplication(MainThreadMarker::new().expect("main thread"));
    while end.timeIntervalSinceNow() > 0.0 {
        autoreleasepool(|_| {
            // Waiting services the run loop (including the GCD clock), but
            // AppKit input must also be dequeued and dispatched to its view.
            if let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&end),
                unsafe { NSDefaultRunLoopMode },
                true,
            ) {
                app.sendEvent(&event);
            }
            app.updateWindows();
        });
    }
}

#[repr(C)]
struct Timeval {
    sec: i64,
    usec: i32,
    _pad: i32,
}

#[repr(C)]
struct RUsage {
    utime: Timeval,
    stime: Timeval,
    maxrss: i64,
    ixrss: i64,
    idrss: i64,
    isrss: i64,
    minflt: i64,
    majflt: i64,
    nswap: i64,
    inblock: i64,
    oublock: i64,
    msgsnd: i64,
    msgrcv: i64,
    nsignals: i64,
    nvcsw: i64,
    nivcsw: i64,
}

fn cpu_secs() -> f64 {
    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut RUsage) -> i32;
    }
    let mut usage = unsafe { std::mem::zeroed::<RUsage>() };
    let rc = unsafe { getrusage(0, &mut usage) };
    if rc != 0 {
        return 0.0;
    }
    let user = usage.utime.sec as f64 + f64::from(usage.utime.usec) * 1e-6;
    let system = usage.stime.sec as f64 + f64::from(usage.stime.usec) * 1e-6;
    user + system
}

fn window_server_secs() -> Option<f64> {
    let output = std::process::Command::new("ps")
        .args(["-ax", "-o", "time=,comm="])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let line = line.trim();
        if !line.contains("WindowServer") {
            continue;
        }
        let time = line.split_whitespace().next()?;
        return counters::parse_ps_time(time);
    }
    None
}
