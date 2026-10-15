//! Throwaway spike. Measures panels, clocks, window queries, click-through, a Carbon hotkey,
//! and the cost of moving a panel (U2, then the move-cost follow-up).
//!
//! Exits on its own. Default duration is 30 seconds.
//!
//! ```text
//! spike_panels --scenario display-link|gcd|query|click|click-slack|hotkey|paused|recreate|slack|scale|bg-move|commit-check \
//!              --panels N --seconds N --work idle|move|contents|walk|combo \
//!              --leeway-ms N --query full|one --query-hz N --rate-mode range|legacy \
//!              --slack 1|2|4|8 --view layer|plain \
//!              --move-api origin|display-yes|display-no|display-no-flush|same \
//!              --px-per-tick N
//! ```

use std::env;
use std::process::ExitCode;

struct Args {
    scenario: String,
    panels: usize,
    seconds: f64,
    work: String,
    leeway_ms: f64,
    query: String,
    query_hz: f64,
    rate_mode: String,
    transaction: bool,
    slack: u32,
    view: String,
    move_api: String,
    px_per_tick: f64,
}

impl Args {
    fn parse() -> Self {
        let mut args = Self {
            scenario: "display-link".to_string(),
            panels: 1,
            seconds: 30.0,
            work: "idle".to_string(),
            leeway_ms: 1.0,
            query: "full".to_string(),
            query_hz: 0.0,
            rate_mode: "range".to_string(),
            transaction: true,
            slack: 1,
            view: "layer".to_string(),
            move_api: "origin".to_string(),
            px_per_tick: 2.0,
        };
        let mut it = env::args().skip(1);
        while let Some(key) = it.next() {
            match key.as_str() {
                "--help" | "-h" => {
                    println!(
                        "spike_panels --scenario display-link|gcd|query|click|click-slack|hotkey|paused|recreate|slack|scale|bg-move|commit-check --panels N --seconds N --work idle|move|contents|walk|combo --leeway-ms N --query full|one --query-hz N --rate-mode range|legacy --slack 1|2|4|8 --view layer|plain --move-api origin|display-yes|display-no|display-no-flush|same --px-per-tick N"
                    );
                    std::process::exit(0);
                }
                "--scenario" => args.scenario = need(&mut it, &key),
                "--panels" => args.panels = parse_num(&need(&mut it, &key), &key),
                "--seconds" => args.seconds = parse_num(&need(&mut it, &key), &key),
                "--work" => args.work = need(&mut it, &key),
                "--leeway-ms" => args.leeway_ms = parse_num(&need(&mut it, &key), &key),
                "--query" => args.query = need(&mut it, &key),
                "--query-hz" => args.query_hz = parse_num(&need(&mut it, &key), &key),
                "--rate-mode" => args.rate_mode = need(&mut it, &key),
                "--no-transaction" => args.transaction = false,
                "--slack" => args.slack = parse_num(&need(&mut it, &key), &key),
                "--view" => args.view = need(&mut it, &key),
                "--move-api" => args.move_api = need(&mut it, &key),
                "--px-per-tick" => args.px_per_tick = parse_num(&need(&mut it, &key), &key),
                other => {
                    eprintln!("unknown argument {other}");
                    std::process::exit(2);
                }
            }
        }
        if args.seconds <= 0.0 {
            eprintln!("--seconds must be > 0");
            std::process::exit(2);
        }
        if !matches!(args.slack, 1 | 2 | 4 | 8) {
            eprintln!("--slack must be 1, 2, 4 or 8");
            std::process::exit(2);
        }
        if args.view != "layer" && args.view != "plain" {
            eprintln!("--view must be layer or plain");
            std::process::exit(2);
        }
        if args.px_per_tick <= 0.0 {
            eprintln!("--px-per-tick must be > 0");
            std::process::exit(2);
        }
        if args.view == "plain" && args.slack > 1 {
            eprintln!("--view plain cannot host a slack sublayer");
            std::process::exit(2);
        }
        args
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

fn main() -> ExitCode {
    let args = Args::parse();
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("spike_panels is macOS-only (scenario {})", args.scenario);
        ExitCode::from(2)
    }
    #[cfg(target_os = "macos")]
    {
        ExitCode::from(macos::run(&args))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::hint::black_box;
    use std::io::{self, Write};
    use std::os::unix::process::ExitStatusExt;
    use std::panic::AssertUnwindSafe;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::runtime::{AnyClass, AnyObject, NSObjectProtocol};
    use objc2::{AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEvent,
        NSEventMask, NSFloatingWindowLevel, NSPanel, NSScreen, NSWindow, NSWindowAnimationBehavior,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
        CGImageAlphaInfo, CGWindowID, CGWindowListCopyWindowInfo, CGWindowListOption,
        kCGNullWindowID, kCGWindowBounds, kCGWindowLayer, kCGWindowMemoryUsage, kCGWindowNumber,
        kCGWindowOwnerPID,
    };
    use objc2_foundation::{
        NSDate, NSDictionary, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString,
    };
    use objc2_quartz_core::{CADisplayLink, CAFrameRateRange, CALayer, CATransaction};

    use super::Args;

    const CELL: f64 = 128.0;
    const TICK_NS: u64 = 20_000_000;

    fn ns_u64(duration: Duration) -> u64 {
        u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
    }

    static MONITOR_EVENTS: AtomicU64 = AtomicU64::new(0);

    struct PanelSlot {
        window: Retained<NSPanel>,
        /// Root layer of a layer-backed content view. None when `--view plain`.
        layer: Option<Retained<CALayer>>,
        /// Sprite sublayer inside a slack panel. None when the sprite fills the window.
        sprite: Option<Retained<CALayer>>,
        base: NSPoint,
        size: f64,
        world_x: f64,
        world_y: f64,
        win_x: f64,
        win_y: f64,
        dir: f64,
    }

    struct Inner {
        panels: Vec<PanelSlot>,
        images: Vec<CFImage>,
        work: Work,
        acc_ns: u64,
        last: Option<Instant>,
        callbacks: u64,
        ticks: u64,
        resyncs: u64,
        /// Window-origin calls (`setFrameOrigin` or `setFrame:display:`).
        moves: u64,
        contents_sets: u64,
        move_ns: u64,
        contents_ns: u64,
        interval_ns_sum: u64,
        interval_ns_min: u64,
        interval_ns_max: u64,
        link_duration_sum: f64,
        link_duration_n: u64,
        phase: u64,
        armed: bool,
        transaction: bool,
        move_api: MoveApi,
        /// Slack scenario: 2 px/tick along x, bounce inside the visible frame.
        steady_walk: bool,
        slack: u32,
        px_per_tick: f64,
        visible: NSRect,
        layer_moves: u64,
        layer_ns: u64,
        step_ns: u64,
        tick_ns: u64,
        clamped: u64,
        flushes: u64,
        flush_ns: u64,
    }

    struct CFImage {
        image: CFRetained<CGImage>,
    }

    impl CFImage {
        fn as_id(&self) -> &AnyObject {
            // CALayer.contents takes a CGImage bridged to id. CGImage is not an NSObject.
            unsafe { CFRetained::as_ptr(&self.image).cast::<AnyObject>().as_ref() }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Work {
        Idle,
        Move,
        Contents,
        Walk,
        /// Window move and a contents swap on every tick.
        Combo,
    }

    impl Work {
        fn parse(value: &str) -> Self {
            match value {
                "idle" => Self::Idle,
                "move" => Self::Move,
                "contents" => Self::Contents,
                "walk" => Self::Walk,
                "combo" => Self::Combo,
                other => {
                    eprintln!("unknown --work {other}");
                    std::process::exit(2);
                }
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::Idle => "idle",
                Self::Move => "move",
                Self::Contents => "contents",
                Self::Walk => "walk",
                Self::Combo => "combo",
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum MoveApi {
        Origin,
        DisplayYes,
        DisplayNo,
        /// `setFrame:display:NO` followed by `displayIfNeeded`.
        DisplayNoFlush,
        /// `setFrameOrigin` of the origin the window already has.
        Same,
    }

    impl MoveApi {
        fn parse(value: &str) -> Self {
            match value {
                "origin" => Self::Origin,
                "display-yes" => Self::DisplayYes,
                "display-no" => Self::DisplayNo,
                "display-no-flush" => Self::DisplayNoFlush,
                "same" => Self::Same,
                other => {
                    eprintln!("unknown --move-api {other}");
                    std::process::exit(2);
                }
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::Origin => "origin",
                Self::DisplayYes => "display-yes",
                Self::DisplayNo => "display-no",
                Self::DisplayNoFlush => "display-no-flush",
                Self::Same => "same",
            }
        }
    }

    struct TickConfig {
        work: Work,
        transaction: bool,
        move_api: MoveApi,
        steady_walk: bool,
        slack: u32,
        px_per_tick: f64,
        visible: NSRect,
    }

    fn new_inner(panels: Vec<PanelSlot>, images: Vec<CFImage>, cfg: TickConfig) -> Inner {
        Inner {
            panels,
            images,
            work: cfg.work,
            acc_ns: 0,
            last: None,
            callbacks: 0,
            ticks: 0,
            resyncs: 0,
            moves: 0,
            contents_sets: 0,
            move_ns: 0,
            contents_ns: 0,
            interval_ns_sum: 0,
            interval_ns_min: u64::MAX,
            interval_ns_max: 0,
            link_duration_sum: 0.0,
            link_duration_n: 0,
            phase: 0,
            armed: false,
            transaction: cfg.transaction,
            move_api: cfg.move_api,
            steady_walk: cfg.steady_walk,
            slack: cfg.slack,
            px_per_tick: cfg.px_per_tick,
            visible: cfg.visible,
            layer_moves: 0,
            layer_ns: 0,
            step_ns: 0,
            tick_ns: 0,
            clamped: 0,
            flushes: 0,
            flush_ns: 0,
        }
    }

    struct Ivars {
        inner: RefCell<Inner>,
    }

    define_class!(
        #[unsafe(super(objc2::runtime::NSObject))]
        #[ivars = Ivars]
        struct SpikeClock;

        impl SpikeClock {
            // `duration` is safe to read here. The earlier abort was
            // `preferredFramesPerSecond` on `_NSDisplayLink`, not this argument.
            #[unsafe(method(onDisplayLink:))]
            fn on_display_link(&self, link: &CADisplayLink) {
                self.step(Some(link.duration()));
            }
        }

        unsafe impl NSObjectProtocol for SpikeClock {}
    );

    impl SpikeClock {
        fn new(inner: Inner) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Ivars {
                inner: RefCell::new(inner),
            });
            unsafe { msg_send![super(this), init] }
        }

        fn step(&self, link_duration: Option<f64>) {
            autoreleasepool(|_| {
                let entered = Instant::now();
                let mut g = self.ivars().inner.borrow_mut();
                if !g.armed {
                    return;
                }
                let now = Instant::now();
                g.callbacks += 1;
                if let Some(prev) = g.last {
                    let dt = ns_u64(now.duration_since(prev));
                    g.interval_ns_sum += dt;
                    g.interval_ns_min = g.interval_ns_min.min(dt);
                    g.interval_ns_max = g.interval_ns_max.max(dt);
                    // Cap a stalled callback so the first gap after a sleep doesn't look like a resync storm.
                    let capped = dt.min(100_000_000);
                    g.acc_ns += capped;
                }
                g.last = Some(now);
                if let Some(dur) = link_duration {
                    g.link_duration_sum += dur;
                    g.link_duration_n += 1;
                }
                let mut fired = 0;
                while g.acc_ns >= TICK_NS && fired < 2 {
                    g.acc_ns -= TICK_NS;
                    fired += 1;
                    tick(&mut g);
                }
                if g.acc_ns >= TICK_NS.saturating_mul(2) {
                    g.acc_ns = 0;
                    g.resyncs += 1;
                }
                g.step_ns += ns_u64(entered.elapsed());
            });
        }

        fn arm(&self) {
            let mut g = self.ivars().inner.borrow_mut();
            g.callbacks = 0;
            g.ticks = 0;
            g.resyncs = 0;
            g.moves = 0;
            g.contents_sets = 0;
            g.move_ns = 0;
            g.contents_ns = 0;
            g.interval_ns_sum = 0;
            g.interval_ns_min = u64::MAX;
            g.interval_ns_max = 0;
            g.link_duration_sum = 0.0;
            g.link_duration_n = 0;
            g.acc_ns = 0;
            g.last = None;
            g.phase = 0;
            g.layer_moves = 0;
            g.layer_ns = 0;
            g.step_ns = 0;
            g.tick_ns = 0;
            g.clamped = 0;
            g.flushes = 0;
            g.flush_ns = 0;
            g.armed = true;
        }

        fn disarm_and_snapshot(&self) -> Inner {
            let mut g = self.ivars().inner.borrow_mut();
            g.armed = false;
            Inner {
                panels: Vec::new(),
                images: Vec::new(),
                work: g.work,
                acc_ns: g.acc_ns,
                last: g.last,
                callbacks: g.callbacks,
                ticks: g.ticks,
                resyncs: g.resyncs,
                moves: g.moves,
                contents_sets: g.contents_sets,
                move_ns: g.move_ns,
                contents_ns: g.contents_ns,
                interval_ns_sum: g.interval_ns_sum,
                interval_ns_min: g.interval_ns_min,
                interval_ns_max: g.interval_ns_max,
                link_duration_sum: g.link_duration_sum,
                link_duration_n: g.link_duration_n,
                phase: g.phase,
                armed: false,
                transaction: g.transaction,
                move_api: g.move_api,
                steady_walk: g.steady_walk,
                slack: g.slack,
                px_per_tick: g.px_per_tick,
                visible: g.visible,
                layer_moves: g.layer_moves,
                layer_ns: g.layer_ns,
                step_ns: g.step_ns,
                tick_ns: g.tick_ns,
                clamped: g.clamped,
                flushes: g.flushes,
                flush_ns: g.flush_ns,
            }
        }
    }

    fn tick(g: &mut Inner) {
        let started = Instant::now();
        g.ticks += 1;
        g.phase += 1;
        let phase = g.phase;
        let work = g.work;
        let move_this = matches!(work, Work::Move | Work::Walk | Work::Combo);
        let contents_this = match work {
            Work::Contents | Work::Combo => true,
            Work::Walk => phase % 6 == 0,
            Work::Idle | Work::Move => false,
        };
        if move_this {
            if g.steady_walk {
                move_steady(g);
            } else {
                move_legacy(g, phase);
            }
        }
        if contents_this {
            swap_contents(g, phase);
        }
        g.tick_ns += ns_u64(started.elapsed());
    }

    fn move_legacy(g: &mut Inner, phase: u64) {
        let api = g.move_api;
        let dx = {
            let step = phase % 80;
            if step < 40 {
                step as f64
            } else {
                80.0 - step as f64
            }
        };
        let n = g.panels.len() as u64;
        if n == 0 {
            return;
        }
        if g.transaction {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
        }
        if api == MoveApi::DisplayNoFlush {
            let started = Instant::now();
            for panel in &g.panels {
                let origin = legacy_origin(panel, dx, api);
                let rect = window_rect(origin, panel.size);
                panel.window.setFrame_display(rect, false);
            }
            g.move_ns += ns_u64(started.elapsed());
            let started = Instant::now();
            for panel in &g.panels {
                panel.window.displayIfNeeded();
            }
            g.flush_ns += ns_u64(started.elapsed());
            g.flushes += n;
        } else {
            let started = Instant::now();
            for panel in &g.panels {
                let origin = legacy_origin(panel, dx, api);
                apply_move(&panel.window, origin, panel.size, api);
            }
            g.move_ns += ns_u64(started.elapsed());
        }
        if g.transaction {
            CATransaction::commit();
        }
        g.moves += n;
    }

    fn legacy_origin(panel: &PanelSlot, dx: f64, api: MoveApi) -> NSPoint {
        if api == MoveApi::Same {
            return panel.base;
        }
        let mut origin = panel.base;
        origin.x += dx;
        origin
    }

    fn window_rect(origin: NSPoint, size: f64) -> NSRect {
        NSRect {
            origin,
            size: NSSize {
                width: size,
                height: size,
            },
        }
    }

    fn apply_move(window: &NSPanel, origin: NSPoint, size: f64, api: MoveApi) {
        match api {
            MoveApi::Origin | MoveApi::Same => window.setFrameOrigin(origin),
            MoveApi::DisplayYes => window.setFrame_display(window_rect(origin, size), true),
            MoveApi::DisplayNo | MoveApi::DisplayNoFlush => {
                window.setFrame_display(window_rect(origin, size), false);
            }
        }
    }

    /// 2 px/tick along x. The layer tracks the sprite. The window moves only when the sprite
    /// would leave it, and then only far enough to put the full slack back in front of the sprite.
    fn move_steady(g: &mut Inner) {
        let px = g.px_per_tick;
        let cell = CELL;
        let min_x = g.visible.origin.x;
        let max_x = g.visible.origin.x + g.visible.size.width;
        for panel in &mut g.panels {
            panel.world_x += px * panel.dir;
            if panel.world_x + cell > max_x {
                panel.world_x = max_x - cell;
                panel.dir = -1.0;
            } else if panel.world_x < min_x {
                panel.world_x = min_x;
                panel.dir = 1.0;
            }
        }
        if g.slack > 1 {
            let started = Instant::now();
            for panel in &g.panels {
                let Some(sprite) = panel.sprite.as_ref() else {
                    continue;
                };
                let local_x = panel.world_x - panel.win_x;
                let local_y = panel.world_y - panel.win_y;
                set_layer_frame(sprite, local_x, local_y, cell, cell);
            }
            g.layer_ns += ns_u64(started.elapsed());
            g.layer_moves += g.panels.len() as u64;
        }
        let slack = g.slack;
        let mut pending: Vec<(usize, NSPoint)> = Vec::new();
        for (index, panel) in g.panels.iter_mut().enumerate() {
            let local_x = panel.world_x - panel.win_x;
            let leave = slack <= 1 || local_x < -0.01 || local_x + cell > panel.size + 0.01;
            if !leave {
                continue;
            }
            if slack <= 1 {
                panel.win_x = panel.world_x;
                panel.win_y = panel.world_y;
            } else if panel.dir > 0.0 {
                panel.win_x = panel.world_x;
            } else {
                panel.win_x = panel.world_x - (panel.size - cell);
            }
            pending.push((
                index,
                NSPoint {
                    x: panel.win_x,
                    y: panel.win_y,
                },
            ));
        }
        if pending.is_empty() {
            return;
        }
        let api = g.move_api;
        let started = Instant::now();
        for (index, origin) in &pending {
            let panel = &g.panels[*index];
            apply_move(&panel.window, *origin, panel.size, api);
        }
        g.move_ns += ns_u64(started.elapsed());
        g.moves += pending.len() as u64;
        for (index, origin) in &pending {
            let got = g.panels[*index].window.frame().origin;
            if (got.x - origin.x).abs() > 0.5 || (got.y - origin.y).abs() > 0.5 {
                g.clamped += 1;
                g.panels[*index].win_x = got.x;
                g.panels[*index].win_y = got.y;
            }
        }
    }

    fn swap_contents(g: &mut Inner, phase: u64) {
        if g.images.is_empty() {
            return;
        }
        let image = &g.images[(phase as usize) % g.images.len()];
        let started = Instant::now();
        if g.transaction {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
        }
        let mut sets = 0u64;
        for panel in &g.panels {
            let Some(layer) = panel.sprite.as_ref().or(panel.layer.as_ref()) else {
                continue;
            };
            unsafe { layer.setContents(Some(image.as_id())) };
            sets += 1;
        }
        if g.transaction {
            CATransaction::commit();
        }
        g.contents_ns += ns_u64(started.elapsed());
        g.contents_sets += sets;
    }

    fn set_layer_frame(layer: &CALayer, x: f64, y: f64, w: f64, h: f64) {
        let frame = CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
        unsafe {
            let _: () = msg_send![layer, setFrame: frame];
        }
    }

    pub fn run(args: &Args) -> u8 {
        println!("pid={}", std::process::id());
        print_host();
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            eprintln!("not on the main thread");
            return 1;
        };
        let app = NSApplication::sharedApplication(mtm);
        app.finishLaunching();
        let policy_ok = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        println!("activation_policy_accessory={policy_ok}");
        print_screens(mtm);

        match args.scenario.as_str() {
            "display-link" => live(mtm, args, Clock::DisplayLink),
            "gcd" => live(mtm, args, Clock::Gcd),
            "paused" => live(mtm, args, Clock::None),
            "recreate" => recreate(mtm, args),
            "query" => query(mtm, args),
            "click" => click(mtm, args),
            "click-slack" => click_slack(mtm, args),
            "hotkey" => hotkey(),
            "slack" => live(mtm, args, Clock::Gcd),
            "scale" => scale(mtm, args),
            "bg-move" => bg_move(mtm, args),
            "commit-check" => commit_check(mtm, args),
            other => {
                eprintln!("unknown --scenario {other}");
                2
            }
        }
    }

    enum Clock {
        DisplayLink,
        Gcd,
        None,
    }

    fn live(mtm: objc2::MainThreadMarker, args: &Args, clock: Clock) -> u8 {
        let mut work = Work::parse(&args.work);
        // `--scenario slack` with no `--work` would otherwise sit idle. A slack run is a walk.
        if args.scenario == "slack" && work == Work::Idle {
            work = Work::Walk;
            println!("slack_default_work=walk");
        }
        let screen = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen,
            None => {
                eprintln!("no main screen");
                return 1;
            }
        };
        let images = match make_images() {
            Some(images) => images,
            None => {
                eprintln!("CGImageCreate failed");
                return 1;
            }
        };
        let move_api = MoveApi::parse(&args.move_api);
        let steady = args.scenario == "slack";
        let window_size = CELL * f64::from(args.slack);
        let sprite = if args.slack > 1 { Some(CELL) } else { None };
        let layer_backed = args.view != "plain";
        let mut panels = make_panels_with(
            mtm,
            &screen,
            args.panels,
            window_size,
            &images[0],
            layer_backed,
            sprite,
        );
        if steady && args.slack > 1 {
            stagger_slack(&mut panels);
        }
        let visible = screen.visibleFrame();
        println!("panels_created={}", panels.len());
        println!("transaction={}", args.transaction);
        println!("move_api={}", move_api.name());
        println!("view={}", args.view);
        println!("slack={}", args.slack);
        println!("window_size={window_size}");
        println!("steady_walk={steady}");
        println!("px_per_tick={}", args.px_per_tick);
        println!(
            "nominal_backing_bytes={}",
            nominal_backing(panels.len(), window_size, screen.backingScaleFactor())
        );
        let clock_obj = SpikeClock::new(new_inner(
            panels,
            images,
            TickConfig {
                work,
                transaction: args.transaction,
                move_api,
                steady_walk: steady,
                slack: args.slack,
                px_per_tick: args.px_per_tick,
                visible,
            },
        ));

        let mut link: Option<Retained<CADisplayLink>> = None;
        let mut gcd: Option<GcdTimer> = None;
        match clock {
            Clock::DisplayLink => {
                link = Some(attach_link(&screen, &clock_obj, &args.rate_mode));
                print_link_range(link.as_ref().unwrap());
            }
            Clock::Gcd => {
                gcd = Some(start_gcd(&clock_obj, args.leeway_ms));
                println!("gcd_interval_ms=20");
                println!("gcd_leeway_ms={}", args.leeway_ms);
            }
            Clock::None => {
                println!("clock=none");
            }
        }
        pump(0.4);
        let listed = copy_windows(
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
            kCGNullWindowID,
            true,
        );
        println!("warmup_window_count={}", listed.count);
        println!("warmup_our_windows={}", listed.ours);
        print_our_window_memory();
        clock_obj.arm();
        ready();
        let t0 = Instant::now();
        let cpu0 = cpu_secs();
        pump(args.seconds);
        let sample = finish_sample(t0, cpu0);
        let snap = clock_obj.disarm_and_snapshot();
        if let Some(link) = link.take() {
            link.invalidate();
        }
        drop(gcd);
        if args.scenario == "slack" {
            probe_edge(&clock_obj, &screen);
        }
        print_snap(&snap, &sample, args);
        done();
        0
    }

    fn recreate(mtm: objc2::MainThreadMarker, args: &Args) -> u8 {
        let screen = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen,
            None => {
                eprintln!("no main screen");
                return 1;
            }
        };
        let images = match make_images() {
            Some(images) => images,
            None => return 1,
        };
        let panels = make_panels(mtm, &screen, 1, CELL, &images[0]);
        let visible = screen.visibleFrame();
        let clock_obj = SpikeClock::new(new_inner(
            panels,
            images,
            TickConfig {
                work: Work::Idle,
                transaction: args.transaction,
                move_api: MoveApi::Origin,
                steady_walk: false,
                slack: 1,
                px_per_tick: args.px_per_tick,
                visible,
            },
        ));
        println!("recreate_note=in-process invalidate+recreate, not a display reconfiguration");
        for i in 1..=5 {
            let link = attach_link(&screen, &clock_obj, &args.rate_mode);
            pump(0.25);
            link.invalidate();
            println!("recreate_pass={i}");
        }
        let link = attach_link(&screen, &clock_obj, &args.rate_mode);
        print_link_range(&link);
        pump(0.3);
        clock_obj.arm();
        ready();
        let t0 = Instant::now();
        let cpu0 = cpu_secs();
        pump(args.seconds);
        let sample = finish_sample(t0, cpu0);
        let snap = clock_obj.disarm_and_snapshot();
        link.invalidate();
        println!("links_left_running=1");
        print_snap(&snap, &sample, args);
        done();
        0
    }

    fn query(mtm: objc2::MainThreadMarker, args: &Args) -> u8 {
        let screen = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen,
            None => return 1,
        };
        let images = match make_images() {
            Some(images) => images,
            None => return 1,
        };
        let size = if args.panels > 50 { 16.0 } else { CELL };
        let panels = make_panels(mtm, &screen, args.panels, size, &images[0]);
        pump(0.5);
        let full_opt =
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
        let census = copy_windows(full_opt, kCGNullWindowID, true);
        println!("panels_created={}", panels.len());
        println!("window_count={}", census.count);
        println!("our_windows={}", census.ours);
        println!(
            "foreign_windows={}",
            census.count.saturating_sub(census.ours)
        );
        let target = if args.query == "one" {
            let id = panels
                .first()
                .map(|p| u32::try_from(p.window.windowNumber()).unwrap_or(0))
                .filter(|id| *id != 0)
                .or(census.sample_foreign)
                .or(census.sample_any);
            match id {
                Some(id) => id,
                None => {
                    eprintln!("no window id to query");
                    done();
                    return 1;
                }
            }
        } else {
            0
        };
        if args.query == "one" {
            println!("query_window_id={target}");
        }
        println!("query_kind={}", args.query);
        println!("query_hz={}", args.query_hz);
        let option = if args.query == "one" {
            CGWindowListOption::OptionIncludingWindow
        } else {
            full_opt
        };
        let relative = if args.query == "one" {
            target
        } else {
            kCGNullWindowID
        };
        ready();
        let t0 = Instant::now();
        let cpu0 = cpu_secs();
        let (calls, ns) = if args.query_hz <= 0.0 {
            tight_query(option, relative, args.seconds)
        } else {
            paced_query(option, relative, args.seconds, args.query_hz)
        };
        let sample = finish_sample(t0, cpu0);
        println!("query_calls={calls}");
        println!(
            "query_us_per_call={:.3}",
            if calls == 0 {
                0.0
            } else {
                ns as f64 / calls as f64 / 1_000.0
            }
        );
        print_cpu(&sample);
        // Keep panels alive until the sample is done.
        drop(panels);
        done();
        0
    }

    fn tight_query(option: CGWindowListOption, relative: CGWindowID, seconds: f64) -> (u64, u64) {
        let start = Instant::now();
        let deadline = Duration::from_secs_f64(seconds);
        let mut calls = 0u64;
        let mut ns = 0u64;
        while start.elapsed() < deadline {
            let t = Instant::now();
            let listed = copy_windows(option, relative, true);
            ns += ns_u64(t.elapsed());
            calls += 1;
            black_box(listed.count);
            black_box(listed.id_sum);
        }
        (calls, ns)
    }

    fn paced_query(
        option: CGWindowListOption,
        relative: CGWindowID,
        seconds: f64,
        hz: f64,
    ) -> (u64, u64) {
        let start = Instant::now();
        let deadline = Duration::from_secs_f64(seconds);
        let period = Duration::from_secs_f64(1.0 / hz);
        let mut next = start;
        let mut calls = 0u64;
        let mut ns = 0u64;
        while start.elapsed() < deadline {
            let t = Instant::now();
            let listed = copy_windows(option, relative, true);
            ns += ns_u64(t.elapsed());
            calls += 1;
            black_box(listed.count);
            next += period;
            let now = Instant::now();
            if next > now && start.elapsed() < deadline {
                pump_until(next);
            }
        }
        (calls, ns)
    }

    fn click(mtm: objc2::MainThreadMarker, args: &Args) -> u8 {
        let screen = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen,
            None => return 1,
        };
        let frame = screen.visibleFrame();
        let origin = NSPoint {
            x: frame.origin.x + 80.0,
            y: frame.origin.y + 80.0,
        };
        let image = match make_alpha_image() {
            Some(image) => image,
            None => return 1,
        };
        let panels = make_panels_at(mtm, &screen, &[(origin, CELL)], &image, true, None);
        let panel = &panels[0];
        pump(0.4);
        let window_id = panel.window.windowNumber();
        println!("click_window_number={window_id}");
        println!(
            "click_frame={:.1},{:.1},{:.1},{:.1}",
            origin.x, origin.y, CELL, CELL
        );
        let opaque = NSPoint {
            x: origin.x + CELL / 2.0,
            y: origin.y + CELL / 2.0,
        };
        let clear = NSPoint {
            x: origin.x + 4.0,
            y: origin.y + 4.0,
        };
        let outside = NSPoint {
            x: origin.x - 30.0,
            y: origin.y - 30.0,
        };
        // Scheme A: read the flag, never write it.
        println!(
            "scheme_a_ignores_mouse_events_before_set={}",
            panel.window.ignoresMouseEvents()
        );
        pump(0.15);
        probe("scheme_a", mtm, opaque, clear, outside, window_id);

        // Explicit set to false, which the research note says disables alpha hit-testing.
        panel.window.setIgnoresMouseEvents(false);
        println!("after_set_false={}", panel.window.ignoresMouseEvents());
        pump(0.2);
        probe("set_false", mtm, opaque, clear, outside, window_id);

        panel.window.setIgnoresMouseEvents(true);
        println!("after_set_true={}", panel.window.ignoresMouseEvents());
        pump(0.2);
        probe("set_true", mtm, opaque, clear, outside, window_id);

        // Scheme B: per-tick sample. The cursor here is synthetic so we don't move the real pointer.
        // Over an opaque pixel the mask says "hit", so the flag is cleared.
        // Over a clear pixel the mask says "miss", so the flag is set.
        let mask = alpha_mask();
        let over_opaque = mask_hit(&mask, 128, 64, 64);
        let over_clear = mask_hit(&mask, 128, 4, 4);
        println!("mask_center_opaque={over_opaque}");
        println!("mask_corner_opaque={over_clear}");
        panel.window.setIgnoresMouseEvents(!over_opaque);
        pump(0.15);
        println!(
            "scheme_b_over_opaque_ignores={}",
            panel.window.ignoresMouseEvents()
        );
        probe(
            "scheme_b_over_opaque",
            mtm,
            opaque,
            clear,
            outside,
            window_id,
        );
        panel.window.setIgnoresMouseEvents(!over_clear);
        pump(0.15);
        println!(
            "scheme_b_over_clear_ignores={}",
            panel.window.ignoresMouseEvents()
        );
        probe(
            "scheme_b_over_clear",
            mtm,
            opaque,
            clear,
            outside,
            window_id,
        );

        let lookups = 200_000u64;
        let t = Instant::now();
        let mut hits = 0u64;
        for i in 0..lookups {
            let x = (i % 128) as usize;
            let y = ((i / 128) % 128) as usize;
            if mask_hit(&mask, 128, x, y) {
                hits += 1;
            }
        }
        black_box(hits);
        println!(
            "mask_lookup_ns={:.2}",
            t.elapsed().as_nanos() as f64 / lookups as f64
        );

        let mouse_n = 20_000u64;
        let t = Instant::now();
        for _ in 0..mouse_n {
            black_box(NSEvent::mouseLocation());
        }
        println!(
            "mouse_location_us={:.3}",
            t.elapsed().as_nanos() as f64 / mouse_n as f64 / 1_000.0
        );

        MONITOR_EVENTS.store(0, Ordering::Relaxed);
        let block = RcBlock::new(|_event: NonNull<NSEvent>| {
            MONITOR_EVENTS.fetch_add(1, Ordering::Relaxed);
        });
        let mask = NSEventMask::MouseMoved | NSEventMask::LeftMouseDragged;
        let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &block);
        println!("monitor_installed={}", monitor.is_some());
        ready();
        let t0 = Instant::now();
        let cpu0 = cpu_secs();
        pump(args.seconds.max(1.0));
        let sample = finish_sample(t0, cpu0);
        println!("monitor_events={}", MONITOR_EVENTS.load(Ordering::Relaxed));
        if let Some(monitor) = &monitor {
            unsafe { NSEvent::removeMonitor(monitor) };
        }
        print_cpu(&sample);
        drop(panels);
        done();
        0
    }

    fn probe(
        label: &str,
        mtm: objc2::MainThreadMarker,
        opaque: NSPoint,
        clear: NSPoint,
        outside: NSPoint,
        our: isize,
    ) {
        let hit = |point: NSPoint| {
            NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(point, 0, mtm)
        };
        println!("{label}_opaque_hit={}", hit(opaque) == our);
        println!("{label}_opaque_window={}", hit(opaque));
        println!("{label}_clear_hit={}", hit(clear) == our);
        println!("{label}_clear_window={}", hit(clear));
        println!("{label}_outside_hit={}", hit(outside) == our);
        println!("{label}_outside_window={}", hit(outside));
    }

    fn hotkey() -> u8 {
        println!("hotkey_api=RegisterEventHotKey");
        let target = unsafe { GetApplicationEventTarget() };
        println!("event_target_null={}", target.is_null());
        if target.is_null() {
            done();
            return 1;
        }
        // control + option + shift. Carbon modifier bits: control 12, option 11, shift 9.
        let modifiers: u32 = (1 << 12) | (1 << 11) | (1 << 9);
        let candidates: [u32; 3] = [0x50, 0x4F, 0x40]; // F19, F18, F17
        let mut registered = false;
        for (index, key_code) in candidates.iter().copied().enumerate() {
            let mut hotkey: *mut c_void = std::ptr::null_mut();
            let status = unsafe {
                RegisterEventHotKey(
                    key_code,
                    modifiers,
                    EventHotKeyId {
                        signature: u32::from_be_bytes(*b"shj2"),
                        id: 1,
                    },
                    target,
                    0,
                    &mut hotkey,
                )
            };
            println!("hotkey_try={index}");
            println!("hotkey_keycode={key_code}");
            println!("hotkey_modifiers={modifiers}");
            println!("hotkey_status={status}");
            println!("hotkey_ref_null={}", hotkey.is_null());
            if status == 0 && !hotkey.is_null() {
                let unreg = unsafe { UnregisterEventHotKey(hotkey) };
                println!("hotkey_unregister_status={unreg}");
                registered = true;
                break;
            }
        }
        println!("hotkey_registered={registered}");
        println!(
            "hotkey_delivery=not measured; registered and unregistered without a Carbon handler"
        );
        ready();
        done();
        if registered { 0 } else { 1 }
    }

    fn attach_link(
        screen: &NSScreen,
        target: &SpikeClock,
        rate_mode: &str,
    ) -> Retained<CADisplayLink> {
        let any: &AnyObject = unsafe { &*std::ptr::from_ref(target).cast::<AnyObject>() };
        let link = unsafe { screen.displayLinkWithTarget_selector(any, sel!(onDisplayLink:)) };
        match rate_mode {
            "legacy" => {
                // Measured separately: setPreferredFramesPerSecond: throws
                // unrecognized selector on _NSDisplayLink (macOS 15.6.1).
                println!("rate_mode=legacy");
                println!(
                    "legacy_setter=throws unrecognized selector on _NSDisplayLink; not called"
                );
                link.setPreferredFrameRateRange(CAFrameRateRange::new(25.0, 60.0, 50.0));
            }
            "range" => {
                link.setPreferredFrameRateRange(CAFrameRateRange::new(25.0, 60.0, 50.0));
                println!("rate_mode=range");
            }
            _ => {
                link.setPreferredFrameRateRange(CAFrameRateRange::new(25.0, 60.0, 50.0));
                println!("rate_mode=range");
                println!("rate_mode_unknown={rate_mode}");
            }
        }
        let run_loop = NSRunLoop::currentRunLoop();
        unsafe { link.addToRunLoop_forMode(&run_loop, NSRunLoopCommonModes) };
        println!("link_duration_before_fire_s={:.6}", link.duration());
        link
    }

    fn print_link_range(link: &CADisplayLink) {
        let range = link.preferredFrameRateRange();
        println!(
            "preferred_range_min={:.3} preferred_range_max={:.3} preferred_range_preferred={:.3}",
            range.minimum, range.maximum, range.preferred
        );
        // `preferredFramesPerSecond` is deprecated and, on this machine's
        // `_NSDisplayLink` (macOS 15.6.1), the getter throws
        // unrecognized selector. Do not call it.
    }

    struct GcdTimer {
        source: *mut c_void,
    }

    impl Drop for GcdTimer {
        fn drop(&mut self) {
            unsafe {
                dispatch_source_cancel(self.source);
                dispatch_release(self.source);
            }
        }
    }

    fn start_gcd(target: &SpikeClock, leeway_ms: f64) -> GcdTimer {
        unsafe extern "C" {
            static _dispatch_source_type_timer: u8;
            // `dispatch_get_main_queue()` is a macro for `&_dispatch_main_q`, not a function.
            static _dispatch_main_q: u8;
            fn dispatch_source_create(
                type_: *const u8,
                handle: usize,
                mask: usize,
                queue: *mut c_void,
            ) -> *mut c_void;
            fn dispatch_source_set_timer(
                source: *mut c_void,
                start: u64,
                interval: u64,
                leeway: u64,
            );
            fn dispatch_source_set_event_handler_f(
                source: *mut c_void,
                handler: Option<unsafe extern "C" fn(*mut c_void)>,
            );
            fn dispatch_set_context(object: *mut c_void, context: *mut c_void);
            fn dispatch_resume(object: *mut c_void);
        }
        let source = unsafe {
            dispatch_source_create(
                &raw const _dispatch_source_type_timer,
                0,
                0,
                &raw const _dispatch_main_q as *mut c_void,
            )
        };
        assert!(!source.is_null(), "dispatch_source_create failed");
        let leeway = (leeway_ms * 1_000_000.0).max(0.0) as u64;
        unsafe {
            dispatch_set_context(
                source,
                std::ptr::from_ref(target).cast_mut().cast::<c_void>(),
            );
            dispatch_source_set_timer(source, 0, 20_000_000, leeway);
            dispatch_source_set_event_handler_f(source, Some(gcd_handler));
            dispatch_resume(source);
        }
        GcdTimer { source }
    }

    /// `dispatch_source_set_event_handler_f` passes the context pointer, not the source.
    /// `dispatch_get_context` is not a plain C function on this SDK; calling it on the clock
    /// object sends `-_getContext` and throws.
    unsafe extern "C" fn gcd_handler(context: *mut c_void) {
        if context.is_null() {
            return;
        }
        let clock = unsafe { &*context.cast::<SpikeClock>() };
        clock.step(None);
    }

    unsafe extern "C" {
        fn dispatch_source_cancel(source: *mut c_void);
        fn dispatch_release(object: *mut c_void);
    }

    fn make_panels(
        mtm: objc2::MainThreadMarker,
        screen: &NSScreen,
        count: usize,
        size: f64,
        image: &CFImage,
    ) -> Vec<PanelSlot> {
        make_panels_with(mtm, screen, count, size, image, true, None)
    }

    fn make_panels_with(
        mtm: objc2::MainThreadMarker,
        screen: &NSScreen,
        count: usize,
        size: f64,
        image: &CFImage,
        layer_backed: bool,
        sprite_cell: Option<f64>,
    ) -> Vec<PanelSlot> {
        let frame = screen.visibleFrame();
        let cols = ((frame.size.width / size).floor() as usize).max(1);
        let mut placed = Vec::with_capacity(count);
        for i in 0..count {
            let col = i % cols;
            let row = i / cols;
            let mut x = frame.origin.x + col as f64 * size;
            let mut y = frame.origin.y + row as f64 * size;
            // Keep every window intersecting the visible frame so OnScreenOnly includes it.
            let max_x = frame.origin.x + frame.size.width - size;
            let max_y = frame.origin.y + frame.size.height - size;
            if x > max_x {
                x = frame.origin.x + (i as f64 * 3.0) % (frame.size.width - size).max(1.0);
            }
            if y > max_y {
                y = frame.origin.y
                    + ((i / cols) as f64 * 3.0) % (frame.size.height - size).max(1.0);
            }
            placed.push((NSPoint { x, y }, size));
        }
        make_panels_at(mtm, screen, &placed, image, layer_backed, sprite_cell)
    }

    fn make_panels_at(
        mtm: objc2::MainThreadMarker,
        screen: &NSScreen,
        placed: &[(NSPoint, f64)],
        image: &CFImage,
        layer_backed: bool,
        sprite_cell: Option<f64>,
    ) -> Vec<PanelSlot> {
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle;
        let clear = NSColor::clearColor();
        let scale = screen.backingScaleFactor();
        let mut panels = Vec::with_capacity(placed.len());
        for (index, (origin, size)) in placed.iter().copied().enumerate() {
            let rect = NSRect {
                origin,
                size: NSSize {
                    width: size,
                    height: size,
                },
            };
            let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                NSPanel::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            );
            unsafe { panel.setReleasedWhenClosed(false) };
            panel.setOpaque(false);
            panel.setHasShadow(false);
            panel.setBackgroundColor(Some(&clear));
            panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
            panel.setLevel(NSFloatingWindowLevel);
            panel.setCollectionBehavior(behavior);
            panel.setHidesOnDeactivate(false);
            panel.setFloatingPanel(true);
            panel.setBecomesKeyOnlyIfNeeded(true);
            let view = panel.contentView().expect("panel content view");
            let (layer, sprite) = if layer_backed {
                view.setWantsLayer(true);
                let layer = view.layer().expect("panel layer");
                layer.setContentsScale(scale);
                let sprite = if let Some(cell) = sprite_cell {
                    let sprite = CALayer::new();
                    sprite.setContentsScale(scale);
                    disable_layer_actions(&sprite);
                    // Start flush against the left edge. A centered sprite spends the first
                    // half-slack before its first window move; the steady rate is the full slack.
                    set_layer_frame(&sprite, 0.0, (size - cell) / 2.0, cell, cell);
                    unsafe { sprite.setContents(Some(image.as_id())) };
                    layer.addSublayer(&sprite);
                    Some(sprite)
                } else {
                    unsafe { layer.setContents(Some(image.as_id())) };
                    None
                };
                (Some(layer), sprite)
            } else {
                view.setWantsLayer(false);
                (None, None)
            };
            panel.orderFrontRegardless();
            if index == 0 {
                println!("collection_behavior_bits={}", panel.collectionBehavior().0);
                println!("panel_level={}", panel.level());
                println!("panel_opaque={}", panel.isOpaque());
                println!("panel_has_shadow={}", panel.hasShadow());
                println!("panel_animation_behavior={}", panel.animationBehavior().0);
                println!("panel_wants_layer={}", view.wantsLayer());
                if let Some(layer) = &layer {
                    let flipped: bool = unsafe { msg_send![layer, isGeometryFlipped] };
                    let anchor: CGPoint = unsafe { msg_send![layer, anchorPoint] };
                    println!("root_geometry_flipped={flipped}");
                    println!("root_anchor={:.2},{:.2}", anchor.x, anchor.y);
                }
                if let Some(sprite) = &sprite {
                    let anchor: CGPoint = unsafe { msg_send![sprite, anchorPoint] };
                    let frame: CGRect = unsafe { msg_send![sprite, frame] };
                    println!("sprite_anchor={:.2},{:.2}", anchor.x, anchor.y);
                    println!(
                        "sprite_frame={:.1},{:.1},{:.1},{:.1}",
                        frame.origin.x, frame.origin.y, frame.size.width, frame.size.height
                    );
                }
            }
            let cell = sprite_cell.unwrap_or(size);
            panels.push(PanelSlot {
                window: panel,
                layer,
                sprite,
                base: origin,
                size,
                world_x: origin.x,
                world_y: origin.y + (size - cell) / 2.0,
                win_x: origin.x,
                win_y: origin.y,
                dir: 1.0,
            });
        }
        panels
    }

    fn stagger_slack(panels: &mut [PanelSlot]) {
        for (index, panel) in panels.iter_mut().enumerate() {
            let travel = panel.size - CELL;
            if travel <= 1.0 {
                continue;
            }
            let shift = (index as f64 * 17.0) % travel;
            panel.world_x = panel.win_x + shift;
            if let Some(sprite) = &panel.sprite {
                let local_y = panel.world_y - panel.win_y;
                set_layer_frame(sprite, shift, local_y, CELL, CELL);
            }
        }
    }

    fn disable_layer_actions(layer: &CALayer) {
        let Some(null_cls) = AnyClass::get(c"NSNull") else {
            return;
        };
        let null: Retained<AnyObject> = unsafe { msg_send![null_cls, null] };
        let keys = ["position", "bounds", "contents"];
        let key_refs: Vec<Retained<NSString>> =
            keys.iter().map(|key| NSString::from_str(key)).collect();
        let key_args: Vec<&NSString> = key_refs.iter().map(|key| key.as_ref()).collect();
        let obj_args: Vec<&AnyObject> = keys.iter().map(|_| &*null).collect();
        let dict: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&key_args, &obj_args);
        unsafe {
            let _: () = msg_send![layer, setActions: &*dict];
        }
    }

    fn nominal_backing(count: usize, window_size: f64, scale: f64) -> u64 {
        let pixels = window_size * scale;
        let bytes = pixels * pixels * 4.0;
        (bytes * count as f64) as u64
    }

    fn print_our_window_memory() {
        let stats = our_window_memory();
        println!("our_window_memory_bytes={}", stats.bytes);
        println!("our_window_memory_windows={}", stats.windows);
        println!("our_window_memory_missing={}", stats.missing);
    }

    fn make_images() -> Option<Vec<CFImage>> {
        Some(vec![
            make_image(128, 128, [220, 80, 40], 24)?,
            make_image(128, 128, [40, 120, 220], 24)?,
        ])
    }

    fn make_alpha_image() -> Option<CFImage> {
        // Center 64² is opaque. The outer 32px on each side is alpha 0, so corners miss.
        make_image(128, 128, [220, 40, 80], 32)
    }

    fn make_image(width: usize, height: usize, rgb: [u8; 3], inset: usize) -> Option<CFImage> {
        let mut bytes = vec![0u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let inside = x >= inset && y >= inset && x < width - inset && y < height - inset;
                if inside {
                    let i = (y * width + x) * 4;
                    bytes[i] = rgb[0];
                    bytes[i + 1] = rgb[1];
                    bytes[i + 2] = rgb[2];
                    bytes[i + 3] = 255;
                }
            }
        }
        let leaked = Box::leak(bytes.into_boxed_slice());
        unsafe extern "C-unwind" fn no_release(
            _info: *mut c_void,
            _data: NonNull<c_void>,
            _size: usize,
        ) {
        }
        let provider = unsafe {
            CGDataProvider::with_data(
                std::ptr::null_mut(),
                leaked.as_ptr().cast(),
                leaked.len(),
                Some(no_release),
            )
        }?;
        let space = CGColorSpace::new_device_rgb()?;
        let image = unsafe {
            CGImage::new(
                width,
                height,
                8,
                32,
                width * 4,
                Some(&space),
                CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0),
                Some(&provider),
                std::ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }?;
        Some(CFImage { image })
    }

    fn alpha_mask() -> Vec<u8> {
        let width = 128usize;
        let mut mask = vec![0u8; (width * width).div_ceil(8)];
        let inset = 32usize;
        for y in 0..width {
            for x in 0..width {
                let inside = x >= inset && y >= inset && x < width - inset && y < width - inset;
                if inside {
                    let i = y * width + x;
                    mask[i / 8] |= 1 << (i % 8);
                }
            }
        }
        mask
    }

    fn mask_hit(mask: &[u8], width: usize, x: usize, y: usize) -> bool {
        let i = y * width + x;
        mask[i / 8] & (1 << (i % 8)) != 0
    }

    struct WindowCensus {
        count: usize,
        ours: usize,
        sample_any: Option<CGWindowID>,
        sample_foreign: Option<CGWindowID>,
        id_sum: u64,
    }

    fn copy_windows(option: CGWindowListOption, relative: CGWindowID, walk: bool) -> WindowCensus {
        let Some(array) = CGWindowListCopyWindowInfo(option, relative) else {
            return WindowCensus {
                count: 0,
                ours: 0,
                sample_any: None,
                sample_foreign: None,
                id_sum: 0,
            };
        };
        let ptr = std::ptr::from_ref(&*array).cast::<c_void>();
        let count = unsafe { CFArrayGetCount(ptr) };
        let count = usize::try_from(count).unwrap_or(0);
        if !walk {
            return WindowCensus {
                count,
                ours: 0,
                sample_any: None,
                sample_foreign: None,
                id_sum: 0,
            };
        }
        let pid = i64::from(std::process::id());
        let number_key = unsafe { std::ptr::from_ref(kCGWindowNumber).cast::<c_void>() };
        let layer_key = unsafe { std::ptr::from_ref(kCGWindowLayer).cast::<c_void>() };
        let owner_key = unsafe { std::ptr::from_ref(kCGWindowOwnerPID).cast::<c_void>() };
        let mut ours = 0usize;
        let mut sample_any = None;
        let mut sample_foreign = None;
        let mut id_sum = 0u64;
        for index in 0..count {
            let dict = unsafe { CFArrayGetValueAtIndex(ptr, index as isize) };
            if dict.is_null() {
                continue;
            }
            let id = dict_i64(dict, number_key).unwrap_or(0);
            let layer = dict_i64(dict, layer_key).unwrap_or(-1);
            let owner = dict_i64(dict, owner_key).unwrap_or(-1);
            if id > 0 {
                id_sum = id_sum.wrapping_add(id as u64);
                if sample_any.is_none() {
                    sample_any = u32::try_from(id).ok();
                }
            }
            if owner == pid {
                ours += 1;
            } else if sample_foreign.is_none() && layer == 0 && id > 0 {
                sample_foreign = u32::try_from(id).ok();
            }
        }
        WindowCensus {
            count,
            ours,
            sample_any,
            sample_foreign,
            id_sum,
        }
    }

    fn dict_i64(dict: *const c_void, key: *const c_void) -> Option<i64> {
        let value = unsafe { CFDictionaryGetValue(dict, key) };
        if value.is_null() {
            return None;
        }
        let mut out = 0i64;
        let ok = unsafe { CFNumberGetValue(value, 4, (&raw mut out).cast()) };
        if ok { Some(out) } else { None }
    }

    unsafe extern "C" {
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFNumberGetValue(number: *const c_void, kind: isize, value: *mut c_void) -> bool;
        fn CGRectMakeWithDictionaryRepresentation(dict: *const c_void, rect: *mut CGRect) -> bool;
    }

    struct MemStats {
        bytes: i64,
        windows: usize,
        missing: usize,
    }

    fn our_window_memory() -> MemStats {
        let Some(array) = CGWindowListCopyWindowInfo(
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
            kCGNullWindowID,
        ) else {
            return MemStats {
                bytes: 0,
                windows: 0,
                missing: 0,
            };
        };
        let ptr = std::ptr::from_ref(&*array).cast::<c_void>();
        let count = usize::try_from(unsafe { CFArrayGetCount(ptr) }).unwrap_or(0);
        let pid = i64::from(std::process::id());
        let owner_key = unsafe { std::ptr::from_ref(kCGWindowOwnerPID).cast::<c_void>() };
        let mem_key = unsafe { std::ptr::from_ref(kCGWindowMemoryUsage).cast::<c_void>() };
        let mut bytes = 0i64;
        let mut windows = 0usize;
        let mut missing = 0usize;
        for index in 0..count {
            let dict = unsafe { CFArrayGetValueAtIndex(ptr, index as isize) };
            if dict.is_null() || dict_i64(dict, owner_key) != Some(pid) {
                continue;
            }
            windows += 1;
            match dict_i64(dict, mem_key) {
                Some(value) => bytes += value,
                None => missing += 1,
            }
        }
        MemStats {
            bytes,
            windows,
            missing,
        }
    }

    /// Window-server bounds converted into AppKit points (origin at the bottom left).
    /// Also returns the raw CoreGraphics rect (origin at the top left of the main display).
    fn server_frame(window_id: CGWindowID, screen_height: f64) -> Option<(NSRect, CGRect)> {
        let array =
            CGWindowListCopyWindowInfo(CGWindowListOption::OptionIncludingWindow, window_id)?;
        let ptr = std::ptr::from_ref(&*array).cast::<c_void>();
        let count = unsafe { CFArrayGetCount(ptr) };
        if count < 1 {
            return None;
        }
        let number_key = unsafe { std::ptr::from_ref(kCGWindowNumber).cast::<c_void>() };
        let mut dict = std::ptr::null();
        for index in 0..count {
            let candidate = unsafe { CFArrayGetValueAtIndex(ptr, index) };
            if candidate.is_null() {
                continue;
            }
            if dict_i64(candidate, number_key) == Some(i64::from(window_id)) {
                dict = candidate;
                break;
            }
        }
        if dict.is_null() {
            return None;
        }
        let key = unsafe { std::ptr::from_ref(kCGWindowBounds).cast::<c_void>() };
        let value = unsafe { CFDictionaryGetValue(dict, key) };
        if value.is_null() {
            return None;
        }
        let mut raw = CGRect::default();
        let ok = unsafe { CGRectMakeWithDictionaryRepresentation(value, &mut raw) };
        if !ok {
            return None;
        }
        let appkit = NSRect {
            origin: NSPoint {
                x: raw.origin.x,
                y: screen_height - raw.origin.y - raw.size.height,
            },
            size: NSSize {
                width: raw.size.width,
                height: raw.size.height,
            },
        };
        Some((appkit, raw))
    }

    fn probe_edge(clock: &SpikeClock, screen: &NSScreen) {
        let g = clock.ivars().inner.borrow();
        let Some(panel) = g.panels.first() else {
            println!("edge_probe=no-panel");
            return;
        };
        let visible = screen.visibleFrame();
        let screen_height = screen.frame().size.height;
        let size = panel.size;
        let id = u32::try_from(panel.window.windowNumber()).unwrap_or(0);
        println!("edge_window_id={id}");
        println!("edge_panel_size={size:.1}");
        println!(
            "edge_visible={:.1},{:.1},{:.1},{:.1}",
            visible.origin.x, visible.origin.y, visible.size.width, visible.size.height
        );
        let cases = [
            (
                "fully_inside",
                visible.origin.x + 40.0,
                visible.origin.y + 40.0,
            ),
            (
                "sprite_at_right_edge",
                visible.origin.x + visible.size.width - CELL,
                visible.origin.y + 40.0,
            ),
            (
                "window_past_right",
                visible.origin.x + visible.size.width - 10.0,
                visible.origin.y + 40.0,
            ),
            (
                "window_past_top",
                visible.origin.x + 40.0,
                visible.origin.y + visible.size.height - 10.0,
            ),
            (
                "window_past_bottom",
                visible.origin.x + 40.0,
                visible.origin.y - 40.0,
            ),
        ];
        for (name, x, y) in cases {
            let origin = NSPoint { x, y };
            panel.window.setFrameOrigin(origin);
            let got = panel.window.frame().origin;
            let clamped = (got.x - origin.x).abs() > 0.5 || (got.y - origin.y).abs() > 0.5;
            println!("edge_{name}_requested_x={:.2}", origin.x);
            println!("edge_{name}_requested_y={:.2}", origin.y);
            println!("edge_{name}_readback_x={:.2}", got.x);
            println!("edge_{name}_readback_y={:.2}", got.y);
            println!("edge_{name}_clamped={clamped}");
            match server_frame(id, screen_height) {
                Some((appkit, raw)) => {
                    let matches = (appkit.origin.x - got.x).abs() < 1.0
                        && (appkit.origin.y - got.y).abs() < 1.0;
                    println!("edge_{name}_server_x={:.2}", appkit.origin.x);
                    println!("edge_{name}_server_y={:.2}", appkit.origin.y);
                    println!("edge_{name}_server_raw_y={:.2}", raw.origin.y);
                    println!("edge_{name}_server_matches_readback={matches}");
                }
                None => println!("edge_{name}_server=missing"),
            }
        }
        // Left-aligned sprite: its right edge is window.x + CELL. Ask for that edge on the
        // visible right edge, which hangs the slack (size - CELL) off the screen.
        let want_x = visible.origin.x + visible.size.width - CELL;
        panel.window.setFrameOrigin(NSPoint {
            x: want_x,
            y: visible.origin.y + 40.0,
        });
        let got_x = panel.window.frame().origin.x;
        let gap = (visible.origin.x + visible.size.width) - (got_x + CELL);
        println!("edge_sprite_right_gap_px={gap:.2}");
        println!("edge_slack_hang_px={:.2}", size - CELL);
        // Top of the visible frame, sprite flush with the top of the panel.
        let visible_top = visible.origin.y + visible.size.height;
        panel.window.setFrameOrigin(NSPoint {
            x: visible.origin.x + 40.0,
            y: visible_top - size,
        });
        let got_y = panel.window.frame().origin.y;
        let sprite_top = got_y + size;
        println!("edge_sprite_top_gap_px={:.2}", visible_top - sprite_top);
    }

    fn click_slack(mtm: objc2::MainThreadMarker, _args: &Args) -> u8 {
        let screen = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen,
            None => return 1,
        };
        let frame = screen.visibleFrame();
        let image = match make_alpha_image() {
            Some(image) => image,
            None => return 1,
        };
        for slack in [2u32, 4, 8] {
            let window_size = CELL * f64::from(slack);
            let mut origin = NSPoint {
                x: frame.origin.x + 20.0,
                y: frame.origin.y + 20.0,
            };
            if origin.x + window_size > frame.origin.x + frame.size.width {
                origin.x = frame.origin.x;
            }
            if origin.y + window_size > frame.origin.y + frame.size.height {
                origin.y = frame.origin.y;
            }
            let panels = make_panels_at(
                mtm,
                &screen,
                &[(origin, window_size)],
                &image,
                true,
                Some(CELL),
            );
            let margin = (window_size - CELL) / 2.0;
            if let Some(sprite) = &panels[0].sprite {
                set_layer_frame(sprite, margin, margin, CELL, CELL);
            }
            pump(0.35);
            let window = &panels[0].window;
            let window_id = window.windowNumber();
            println!("click_slack={slack}");
            println!("click_window_number={window_id}");
            println!(
                "click_frame={:.1},{:.1},{:.1},{:.1}",
                origin.x, origin.y, window_size, window_size
            );
            println!("click_ignores_mouse_events={}", window.ignoresMouseEvents());
            let sprite_x = origin.x + margin;
            let sprite_y = origin.y + margin;
            let opaque = NSPoint {
                x: sprite_x + CELL / 2.0,
                y: sprite_y + CELL / 2.0,
            };
            let sprite_clear = NSPoint {
                x: sprite_x + 4.0,
                y: sprite_y + 4.0,
            };
            let slack_pt = NSPoint {
                x: origin.x + 4.0,
                y: origin.y + 4.0,
            };
            let outside = NSPoint {
                x: origin.x - 30.0,
                y: origin.y - 30.0,
            };
            let label = format!("slack_{slack}");
            probe(&label, mtm, opaque, sprite_clear, outside, window_id);
            let slack_hit =
                NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(slack_pt, 0, mtm);
            println!("{label}_slack_hit={}", slack_hit == window_id);
            println!("{label}_slack_window={slack_hit}");
            let mut opaque_n = 0u64;
            let mut opaque_hit = 0u64;
            let mut sprite_clear_n = 0u64;
            let mut sprite_clear_hit = 0u64;
            let mut slack_n = 0u64;
            let mut slack_hit_n = 0u64;
            let mut outside_n = 0u64;
            let mut outside_hit_n = 0u64;
            let step = 16.0f64;
            let mut y = origin.y - step;
            while y < origin.y + window_size + step {
                let mut x = origin.x - step;
                while x < origin.x + window_size + step {
                    let point = NSPoint { x, y };
                    let hit =
                        NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(point, 0, mtm)
                            == window_id;
                    let in_window = x >= origin.x
                        && x < origin.x + window_size
                        && y >= origin.y
                        && y < origin.y + window_size;
                    let in_sprite = x >= sprite_x
                        && x < sprite_x + CELL
                        && y >= sprite_y
                        && y < sprite_y + CELL;
                    let in_opaque = x >= sprite_x + 32.0
                        && x < sprite_x + CELL - 32.0
                        && y >= sprite_y + 32.0
                        && y < sprite_y + CELL - 32.0;
                    if in_opaque {
                        opaque_n += 1;
                        if hit {
                            opaque_hit += 1;
                        }
                    } else if in_sprite {
                        sprite_clear_n += 1;
                        if hit {
                            sprite_clear_hit += 1;
                        }
                    } else if in_window {
                        slack_n += 1;
                        if hit {
                            slack_hit_n += 1;
                        }
                    } else {
                        outside_n += 1;
                        if hit {
                            outside_hit_n += 1;
                        }
                    }
                    x += step;
                }
                y += step;
            }
            println!("{label}_grid_opaque={opaque_hit}/{opaque_n}");
            println!("{label}_grid_sprite_clear={sprite_clear_hit}/{sprite_clear_n}");
            println!("{label}_grid_slack={slack_hit_n}/{slack_n}");
            println!("{label}_grid_outside={outside_hit_n}/{outside_n}");
            // A horizontal cut through the sprite. Offsets 0 and 16 sit in the image's
            // alpha-0 inset; 32 and 64 sit in the opaque centre.
            for offset in [0.0, 4.0, 16.0, 31.0, 32.0, 64.0, 96.0, 112.0, 127.0] {
                let point = NSPoint {
                    x: sprite_x + offset,
                    y: sprite_y + CELL / 2.0,
                };
                let hit = NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(point, 0, mtm);
                println!("{label}_cross_x{offset}_hit={}", hit == window_id);
            }
            drop(panels);
            pump(0.15);
        }
        probe_root_contents(mtm, &screen, &image);
        done();
        0
    }

    /// Same 2x panel, but the transparent margin is pixels of the root layer's image
    /// rather than empty space around a sublayer.
    fn probe_root_contents(mtm: objc2::MainThreadMarker, screen: &NSScreen, _sprite: &CFImage) {
        let window_size = CELL * 2.0;
        let frame = screen.visibleFrame();
        let origin = NSPoint {
            x: frame.origin.x + 20.0,
            y: frame.origin.y + 20.0,
        };
        let Some(image) = make_image(256, 256, [220, 40, 80], 96) else {
            println!("root_contents=image-failed");
            return;
        };
        // inset 96 on a 256 image leaves a 64² opaque centre, matching the alpha sprite,
        // with a clear margin on every side that is part of the same contents image.
        let panels = make_panels_at(mtm, screen, &[(origin, window_size)], &image, true, None);
        pump(0.3);
        let window_id = panels[0].window.windowNumber();
        let centre = NSPoint {
            x: origin.x + window_size / 2.0,
            y: origin.y + window_size / 2.0,
        };
        let image_clear = NSPoint {
            x: origin.x + window_size / 2.0 - 40.0,
            y: origin.y + window_size / 2.0,
        };
        let margin = NSPoint {
            x: origin.x + 4.0,
            y: origin.y + 4.0,
        };
        let outside = NSPoint {
            x: origin.x - 30.0,
            y: origin.y - 30.0,
        };
        let hit = |point: NSPoint| {
            NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(point, 0, mtm) == window_id
        };
        println!("root_contents_window={window_id}");
        println!("root_contents_centre_hit={}", hit(centre));
        println!("root_contents_image_clear_hit={}", hit(image_clear));
        println!("root_contents_margin_hit={}", hit(margin));
        println!("root_contents_outside_hit={}", hit(outside));
        drop(panels);
    }

    fn scale(mtm: objc2::MainThreadMarker, args: &Args) -> u8 {
        let screen = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen,
            None => return 1,
        };
        let images = match make_images() {
            Some(images) => images,
            None => return 1,
        };
        let count = args.panels.max(50);
        let panels = make_panels(mtm, &screen, count, CELL, &images[0]);
        pump(0.4);
        println!("scale_panels_created={}", panels.len());
        println!("scale_phase_seconds={}", args.seconds);
        ready();
        for n in [1usize, 2, 5, 10, 20, 50] {
            if n > panels.len() {
                break;
            }
            time_batches(&panels[..n], args.seconds, "tight", false, 1);
        }
        if let Some(panel) = panels.first() {
            time_repeat_one(panel, 50, args.seconds);
        }
        if panels.len() >= 2 {
            time_first_rest(&panels, args.seconds);
        }
        for n in [1usize, 10, 50] {
            if n > panels.len() {
                break;
            }
            time_batches(&panels[..n], args.seconds, "paced", true, 1);
        }
        drop(panels);
        done();
        0
    }

    fn time_batches(panels: &[PanelSlot], seconds: f64, label: &str, paced: bool, repeat: u64) {
        let start = Instant::now();
        let cpu0 = cpu_secs();
        let deadline = Duration::from_secs_f64(seconds);
        let period = Duration::from_millis(20);
        let mut next = start;
        let mut batches = 0u64;
        let mut ns = 0u64;
        let mut flip = 0.0;
        while start.elapsed() < deadline {
            flip = if flip == 0.0 { 1.0 } else { 0.0 };
            let t = Instant::now();
            for _ in 0..repeat {
                for panel in panels {
                    let mut origin = panel.base;
                    origin.x += flip;
                    panel.window.setFrameOrigin(origin);
                }
            }
            ns += ns_u64(t.elapsed());
            batches += 1;
            if paced {
                next += period;
                let now = Instant::now();
                if next > now && start.elapsed() < deadline {
                    pump_until(next);
                }
            }
        }
        let sample = finish_sample(start, cpu0);
        let calls = batches * panels.len() as u64 * repeat;
        println!("scale_phase={label}");
        println!("scale_n={}", panels.len());
        println!("scale_batches={batches}");
        println!("scale_calls={calls}");
        println!("scale_batch_us={:.3}", per_call_us(ns, batches));
        println!("scale_us_per_call={:.3}", per_call_us(ns, calls));
        print_cpu(&sample);
    }

    fn time_repeat_one(panel: &PanelSlot, repeats: u64, seconds: f64) {
        let start = Instant::now();
        let cpu0 = cpu_secs();
        let deadline = Duration::from_secs_f64(seconds);
        let mut batches = 0u64;
        let mut ns = 0u64;
        while start.elapsed() < deadline {
            let t = Instant::now();
            for i in 0..repeats {
                let mut origin = panel.base;
                origin.x += (i % 2) as f64;
                panel.window.setFrameOrigin(origin);
            }
            ns += ns_u64(t.elapsed());
            batches += 1;
        }
        let sample = finish_sample(start, cpu0);
        let calls = batches * repeats;
        println!("scale_phase=tight-one-window");
        println!("scale_n=1");
        println!("scale_repeats_per_batch={repeats}");
        println!("scale_batches={batches}");
        println!("scale_calls={calls}");
        println!("scale_batch_us={:.3}", per_call_us(ns, batches));
        println!("scale_us_per_call={:.3}", per_call_us(ns, calls));
        print_cpu(&sample);
    }

    fn time_first_rest(panels: &[PanelSlot], seconds: f64) {
        let start = Instant::now();
        let deadline = Duration::from_secs_f64(seconds);
        let mut batches = 0u64;
        let mut first_ns = 0u64;
        let mut rest_ns = 0u64;
        let mut flip = 0.0;
        while start.elapsed() < deadline {
            flip = if flip == 0.0 { 1.0 } else { 0.0 };
            let t = Instant::now();
            let mut origin = panels[0].base;
            origin.x += flip;
            panels[0].window.setFrameOrigin(origin);
            first_ns += ns_u64(t.elapsed());
            let t = Instant::now();
            for panel in &panels[1..] {
                let mut origin = panel.base;
                origin.x += flip;
                panel.window.setFrameOrigin(origin);
            }
            rest_ns += ns_u64(t.elapsed());
            batches += 1;
        }
        let rest_calls = batches * panels[1..].len() as u64;
        println!("scale_phase=tight-first-rest");
        println!("scale_n={}", panels.len());
        println!("scale_batches={batches}");
        println!("scale_first_us={:.3}", per_call_us(first_ns, batches));
        println!("scale_rest_calls={rest_calls}");
        println!(
            "scale_rest_us_per_call={:.3}",
            per_call_us(rest_ns, rest_calls)
        );
        println!("scale_rest_batch_us={:.3}", per_call_us(rest_ns, batches));
    }

    fn bg_move(mtm: objc2::MainThreadMarker, _args: &Args) -> u8 {
        // AppKit traps, it does not throw. A worker process keeps this run self-terminating.
        if std::env::var_os("SPIKE_BG_WORKER").is_none() {
            let exe = match std::env::current_exe() {
                Ok(exe) => exe,
                Err(err) => {
                    eprintln!("bg_exe_error={err}");
                    return 1;
                }
            };
            let mut child = match std::process::Command::new(exe)
                .args(std::env::args().skip(1))
                .env("SPIKE_BG_WORKER", "1")
                .spawn()
            {
                Ok(child) => child,
                Err(err) => {
                    eprintln!("bg_spawn_error={err}");
                    return 1;
                }
            };
            let status = match child.wait() {
                Ok(status) => status,
                Err(err) => {
                    eprintln!("bg_wait_error={err}");
                    return 1;
                }
            };
            println!("bg_worker=child");
            match status.code() {
                Some(code) => println!("bg_worker_exit={code}"),
                None => println!("bg_worker_exit=signal"),
            }
            println!("bg_worker_success={}", status.success());
            if let Some(signal) = status.signal() {
                println!("bg_worker_signal={signal}");
            }
            println!(
                "bg_result={}",
                if status.success() { "ok" } else { "trapped" }
            );
            done();
            return 0;
        }
        let screen = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen,
            None => return 1,
        };
        let images = match make_images() {
            Some(images) => images,
            None => return 1,
        };
        let panels = make_panels(mtm, &screen, 1, CELL, &images[0]);
        pump(0.3);
        let window = &panels[0].window;
        let before = window.frame().origin;
        let target = NSPoint {
            x: before.x + 80.0,
            y: before.y + 40.0,
        };
        // `*mut NSPanel` is not `Send`. The address is the only thing the worker needs.
        let raw = Retained::as_ptr(window) as usize;
        println!("bg_before_x={:.2}", before.x);
        println!("bg_before_y={:.2}", before.y);
        println!("bg_target_x={:.2}", target.x);
        println!("bg_target_y={:.2}", target.y);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let is_main: bool = unsafe {
                match AnyClass::get(c"NSThread") {
                    Some(cls) => msg_send![cls, isMainThread],
                    None => false,
                }
            };
            let caught = objc2::exception::catch(AssertUnwindSafe(|| {
                autoreleasepool(|_| {
                    let obj = unsafe { &*(raw as *mut AnyObject) };
                    let _: () = unsafe { msg_send![obj, setFrameOrigin: target] };
                });
            }));
            let text = match caught {
                Ok(()) => format!("ok main_thread={is_main}"),
                Err(Some(exc)) => format!("exception main_thread={is_main} {exc}"),
                Err(None) => format!("nil-exception main_thread={is_main}"),
            };
            let _ = tx.send(text);
        });
        ready();
        let result = match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(text) => text,
            Err(err) => format!("recv-{err}"),
        };
        pump(0.25);
        let after = window.frame().origin;
        println!("bg_result={result}");
        println!("bg_readback_x={:.2}", after.x);
        println!("bg_readback_y={:.2}", after.y);
        let moved = (after.x - target.x).abs() < 0.5 && (after.y - target.y).abs() < 0.5;
        let unchanged = (after.x - before.x).abs() < 0.5 && (after.y - before.y).abs() < 0.5;
        println!("bg_moved={moved}");
        println!("bg_unchanged={unchanged}");
        window.setFrameOrigin(before);
        let back = window.frame().origin;
        let main_ok = (back.x - before.x).abs() < 0.5 && (back.y - before.y).abs() < 0.5;
        println!("bg_main_still_works={main_ok}");
        drop(panels);
        done();
        0
    }

    fn commit_check(mtm: objc2::MainThreadMarker, _args: &Args) -> u8 {
        let screen = match NSScreen::mainScreen(mtm) {
            Some(screen) => screen,
            None => return 1,
        };
        let images = match make_images() {
            Some(images) => images,
            None => return 1,
        };
        let panels = make_panels(mtm, &screen, 1, CELL, &images[0]);
        pump(0.3);
        let window = &panels[0].window;
        let id = u32::try_from(window.windowNumber()).unwrap_or(0);
        let screen_height = screen.frame().size.height;
        let base = window.frame().origin;
        let size = panels[0].size;
        println!("commit_window_id={id}");
        println!("commit_base_x={:.2}", base.x);
        println!("commit_base_y={:.2}", base.y);
        let apis = [
            ("origin", MoveApi::Origin),
            ("display-yes", MoveApi::DisplayYes),
            ("display-no", MoveApi::DisplayNo),
            ("display-no-flush", MoveApi::DisplayNoFlush),
        ];
        for (name, api) in apis {
            let target = NSPoint {
                x: base.x + 36.0,
                y: base.y + 18.0,
            };
            let started = Instant::now();
            apply_move(window, target, size, api);
            let call_us = ns_u64(started.elapsed()) as f64 / 1_000.0;
            let local = window.frame().origin;
            let server_now = server_frame(id, screen_height);
            pump(0.08);
            let local_after = window.frame().origin;
            let server_after = server_frame(id, screen_height);
            println!("commit_api={name}");
            println!("commit_{name}_call_us={call_us:.3}");
            println!("commit_{name}_local_matches={}", near(local, target));
            println!(
                "commit_{name}_local_after_matches={}",
                near(local_after, target)
            );
            println!(
                "commit_{name}_server_now_matches={}",
                server_now.is_some_and(|(rect, _)| near(rect.origin, target))
            );
            println!(
                "commit_{name}_server_after_matches={}",
                server_after.is_some_and(|(rect, _)| near(rect.origin, target))
            );
            if let Some((rect, raw)) = server_after {
                println!("commit_{name}_server_x={:.2}", rect.origin.x);
                println!("commit_{name}_server_y={:.2}", rect.origin.y);
                println!("commit_{name}_server_raw_y={:.2}", raw.origin.y);
            }
            window.setFrameOrigin(base);
            pump(0.08);
        }
        drop(panels);
        done();
        0
    }

    fn near(got: NSPoint, want: NSPoint) -> bool {
        (got.x - want.x).abs() < 1.0 && (got.y - want.y).abs() < 1.0
    }

    fn pump(seconds: f64) {
        pump_until(Instant::now() + Duration::from_secs_f64(seconds));
    }

    fn pump_until(deadline: Instant) {
        while Instant::now() < deadline {
            let left = deadline
                .saturating_duration_since(Instant::now())
                .as_secs_f64();
            if left <= 0.0 {
                break;
            }
            let slice = left.min(0.05);
            let date = NSDate::dateWithTimeIntervalSinceNow(slice);
            NSRunLoop::currentRunLoop().runUntilDate(&date);
        }
    }

    fn print_screens(mtm: objc2::MainThreadMarker) {
        let screens = NSScreen::screens(mtm);
        println!("screen_count={}", screens.count());
        for index in 0..screens.count() {
            let screen = screens.objectAtIndex(index);
            let frame = screen.frame();
            println!(
                "screen_{index}_frame={:.1},{:.1},{:.1},{:.1}",
                frame.origin.x, frame.origin.y, frame.size.width, frame.size.height
            );
            let visible = screen.visibleFrame();
            println!(
                "screen_{index}_visible={:.1},{:.1},{:.1},{:.1}",
                visible.origin.x, visible.origin.y, visible.size.width, visible.size.height
            );
            println!("screen_{index}_max_fps={}", screen.maximumFramesPerSecond());
            println!(
                "screen_{index}_min_refresh_s={:.6}",
                screen.minimumRefreshInterval()
            );
            println!(
                "screen_{index}_max_refresh_s={:.6}",
                screen.maximumRefreshInterval()
            );
            println!(
                "screen_{index}_granularity_s={:.6}",
                screen.displayUpdateGranularity()
            );
            println!("screen_{index}_scale={}", screen.backingScaleFactor());
        }
    }

    fn print_host() {
        println!("host_os={}", command_line("sw_vers", &["-productVersion"]));
        println!("host_build={}", command_line("sw_vers", &["-buildVersion"]));
        println!(
            "host_cpu={}",
            command_line("sysctl", &["-n", "machdep.cpu.brand_string"])
        );
        println!("host_model={}", command_line("sysctl", &["-n", "hw.model"]));
    }

    fn command_line(cmd: &str, args: &[&str]) -> String {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|text| text.trim().replace(['\n', ' '], "_"))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }

    struct Sample {
        wall: Duration,
        user: f64,
        sys: f64,
        maxrss: i64,
    }

    fn finish_sample(start: Instant, cpu0: (f64, f64)) -> Sample {
        let cpu1 = cpu_secs();
        Sample {
            wall: start.elapsed(),
            user: (cpu1.0 - cpu0.0).max(0.0),
            sys: (cpu1.1 - cpu0.1).max(0.0),
            maxrss: rss_bytes(),
        }
    }

    fn print_cpu(sample: &Sample) {
        let wall = sample.wall.as_secs_f64();
        let pct = if wall > 0.0 {
            (sample.user + sample.sys) / wall * 100.0
        } else {
            0.0
        };
        println!("wall_s={wall:.3}");
        println!("rusage_user_s={:.4}", sample.user);
        println!("rusage_sys_s={:.4}", sample.sys);
        println!("rusage_cpu_percent={pct:.2}");
        println!("rusage_maxrss_bytes={}", sample.maxrss);
    }

    fn print_snap(snap: &Inner, sample: &Sample, args: &Args) {
        let wall = sample.wall.as_secs_f64();
        println!("scenario={}", args.scenario);
        println!("work={}", args.work);
        println!("work_effective={}", snap.work.name());
        println!("panels_requested={}", args.panels);
        println!("move_api={}", snap.move_api.name());
        println!("slack={}", snap.slack);
        println!("steady_walk={}", snap.steady_walk);
        println!("px_per_tick={}", snap.px_per_tick);
        let intervals = snap.callbacks.saturating_sub(1);
        let callback_hz = if wall > 0.0 {
            snap.callbacks as f64 / wall
        } else {
            0.0
        };
        let tick_hz = if wall > 0.0 {
            snap.ticks as f64 / wall
        } else {
            0.0
        };
        println!("callbacks={}", snap.callbacks);
        println!("callback_hz={callback_hz:.3}");
        println!("ticks={}", snap.ticks);
        println!("tick_hz={tick_hz:.3}");
        println!("resyncs={}", snap.resyncs);
        println!("moves={}", snap.moves);
        println!("window_moves={}", snap.moves);
        println!(
            "move_hz={:.3}",
            if wall > 0.0 {
                snap.moves as f64 / wall
            } else {
                0.0
            }
        );
        println!(
            "move_us_per_call={:.3}",
            per_call_us(snap.move_ns, snap.moves)
        );
        println!("contents_sets={}", snap.contents_sets);
        println!(
            "contents_hz={:.3}",
            if wall > 0.0 {
                snap.contents_sets as f64 / wall
            } else {
                0.0
            }
        );
        println!(
            "contents_us_per_call={:.3}",
            per_call_us(snap.contents_ns, snap.contents_sets)
        );
        println!("layer_moves={}", snap.layer_moves);
        println!(
            "layer_hz={:.3}",
            if wall > 0.0 {
                snap.layer_moves as f64 / wall
            } else {
                0.0
            }
        );
        println!(
            "layer_us_per_call={:.3}",
            per_call_us(snap.layer_ns, snap.layer_moves)
        );
        println!("flushes={}", snap.flushes);
        println!(
            "flush_us_per_call={:.3}",
            per_call_us(snap.flush_ns, snap.flushes)
        );
        println!("clamped={}", snap.clamped);
        if snap.callbacks > 0 {
            println!(
                "step_us_per_callback={:.3}",
                per_call_us(snap.step_ns, snap.callbacks)
            );
            println!(
                "callback_overhead_us={:.3}",
                per_call_us(snap.step_ns.saturating_sub(snap.tick_ns), snap.callbacks)
            );
        }
        if snap.ticks > 0 {
            println!(
                "tick_us_per_tick={:.3}",
                per_call_us(snap.tick_ns, snap.ticks)
            );
        }
        if intervals > 0 && snap.interval_ns_min != u64::MAX {
            println!(
                "interval_ms_min={:.3}",
                snap.interval_ns_min as f64 / 1_000_000.0
            );
            println!(
                "interval_ms_avg={:.3}",
                snap.interval_ns_sum as f64 / intervals as f64 / 1_000_000.0
            );
            println!(
                "interval_ms_max={:.3}",
                snap.interval_ns_max as f64 / 1_000_000.0
            );
        }
        if snap.link_duration_n > 0 {
            println!(
                "link_duration_ms_avg={:.3}",
                snap.link_duration_sum / snap.link_duration_n as f64 * 1_000.0
            );
        }
        print_cpu(sample);
    }

    fn per_call_us(total_ns: u64, calls: u64) -> f64 {
        if calls == 0 {
            0.0
        } else {
            total_ns as f64 / calls as f64 / 1_000.0
        }
    }

    fn ready() {
        println!("READY");
        let _ = io::stdout().flush();
    }

    fn done() {
        println!("DONE");
        let _ = io::stdout().flush();
        // The external sampler reads `ps` after DONE, while this process is still alive.
        std::thread::sleep(Duration::from_millis(400));
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

    fn read_rusage() -> Option<RUsage> {
        unsafe extern "C" {
            fn getrusage(who: i32, usage: *mut RUsage) -> i32;
        }
        let mut usage = unsafe { std::mem::zeroed::<RUsage>() };
        let rc = unsafe { getrusage(0, &mut usage) };
        if rc != 0 { None } else { Some(usage) }
    }

    fn cpu_secs() -> (f64, f64) {
        let Some(usage) = read_rusage() else {
            return (0.0, 0.0);
        };
        (
            usage.utime.sec as f64 + f64::from(usage.utime.usec) * 1e-6,
            usage.stime.sec as f64 + f64::from(usage.stime.usec) * 1e-6,
        )
    }

    fn rss_bytes() -> i64 {
        // macOS reports `ru_maxrss` in bytes.
        read_rusage().map(|usage| usage.maxrss).unwrap_or(0)
    }

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn RegisterEventHotKey(
            key_code: u32,
            modifiers: u32,
            hotkey_id: EventHotKeyId,
            target: *mut c_void,
            options: u32,
            out_ref: *mut *mut c_void,
        ) -> i32;
        fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
        fn GetApplicationEventTarget() -> *mut c_void;
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct EventHotKeyId {
        signature: u32,
        id: u32,
    }
}
