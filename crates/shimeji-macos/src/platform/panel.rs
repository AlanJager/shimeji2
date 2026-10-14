//! One slack `NSPanel` per mascot, with cursor-driven per-pixel click-through.

use std::sync::Arc;

use super::hit_test::AlphaMask;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSPanel, NSScreen,
    NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{
    CALayer, CALayerContentsFilter, CATransform3D, CATransform3DIdentity, kCAFilterLinear,
    kCAFilterNearest,
};

use super::coords::{CocoaSpace, Rect, work_area};
use super::image_cache::ImageCache;
use super::input::{InputQueue, PetView};
use super::poses::PoseTable;
use super::render::{CommitCounters, Renderer};
use super::slack::{
    clamp_sprite_x, keep_sprite_inside, next_panel_origin, panel_needs_resize, panel_size,
    placement_anchor, sprite_inside, sprite_origin,
};
use shimeji_core::delta::{MascotDelta, MascotId};
use shimeji_core::engine::MascotFrame;
use shimeji_core::env::Point;

pub const POOL_SIZE: usize = 8;

define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    struct PetPanel;

    impl PetPanel {
        // The slack window may extend beyond the display. Constraining that
        // window shifts the sprite away from its engine coordinate; only the
        // sprite's placement should be constrained by the renderer.
        #[unsafe(method(constrainFrameRect:toScreen:))]
        fn constrain_frame(&self, frame: NSRect, _screen: Option<&NSScreen>) -> NSRect {
            frame
        }
    }
);

pub struct Panel {
    pub window: Retained<NSPanel>,
    sprite: Retained<CALayer>,
    view: Retained<PetView>,
    mascot: MascotId,
    panel: Option<Rect>,
    last_sprite: Option<Rect>,
    last_image: Option<String>,
    last_look_right: bool,
    last_mirrored: bool,
    last_key: Option<String>,
    shown: bool,
    mask: Option<Arc<AlphaMask>>,
    ignores_mouse: Option<bool>,
}

impl Panel {
    pub fn create(
        mtm: MainThreadMarker,
        size: f64,
        mascot: MascotId,
        queue: InputQueue,
        scale: f64,
    ) -> Self {
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle;
        let rect = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: size,
                height: size,
            },
        };
        let window: Retained<PetPanel> = unsafe {
            msg_send![super(PetPanel::alloc(mtm).set_ivars(())),
                initWithContentRect: rect,
                styleMask: style,
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ]
        };
        let window = Retained::into_super(window);
        unsafe { window.setReleasedWhenClosed(false) };
        window.setOpaque(false);
        window.setHasShadow(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        window.setLevel(NSFloatingWindowLevel);
        window.setCollectionBehavior(behavior);
        window.setHidesOnDeactivate(false);
        window.setFloatingPanel(true);
        window.setBecomesKeyOnlyIfNeeded(true);
        window.setAcceptsMouseMovedEvents(true);
        // The child-layer hit region includes transparent pixels. The renderer
        // supplies the alpha-mask policy instead of relying on that region.

        let view = PetView::new(mtm, rect, mascot, queue);
        view.setWantsLayer(true);
        window.setContentView(Some(&view));
        let layer = view.layer().expect("panel layer");
        layer.setContentsScale(scale);
        let sprite = CALayer::new();
        sprite.setContentsScale(1.0);
        disable_layer_actions(&sprite);
        layer.addSublayer(&sprite);

        Self {
            window,
            sprite,
            view,
            mascot,
            panel: None,
            last_sprite: None,
            last_image: None,
            last_look_right: false,
            last_mirrored: false,
            last_key: None,
            shown: false,
            mask: None,
            ignores_mouse: None,
        }
    }

    fn set_sampling(&self, linear: bool) {
        let filter: &CALayerContentsFilter = if linear {
            unsafe { kCAFilterLinear }
        } else {
            unsafe { kCAFilterNearest }
        };
        self.sprite.setMinificationFilter(filter);
        self.sprite.setMagnificationFilter(filter);
    }

    pub fn bind(&self, mascot: MascotId) {
        self.view.set_mascot(mascot);
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }

    pub fn show(&self) {
        self.window.orderFrontRegardless();
    }
}

pub struct PanelRenderer {
    mtm: MainThreadMarker,
    live: Vec<Panel>,
    pool: super::panel_pool::PanelPool,
    cache: ImageCache,
    poses: PoseTable,
    space: CocoaSpace,
    slack: u32,
    queue: InputQueue,
    scale: f64,
    /// User scale from settings. Combined with the mirror; `contentsScale` stays the screen scale.
    user_scale: f64,
    opacity: f64,
    /// Bicubic maps to linear. Nearest and hqx stay nearest (no hqx filter on the layer).
    filter_linear: bool,
    /// Air wall. Off leaves the sprite where the pose anchor places it.
    keep_on_screen: bool,
    /// Next commit repositions every live sprite. Set when the air wall toggles
    /// or the screen list changes, including while a pet is sitting still.
    layout_dirty: bool,
    pub counters: CommitCounters,
}

impl PanelRenderer {
    pub fn new(
        mtm: MainThreadMarker,
        cache: ImageCache,
        poses: PoseTable,
        space: CocoaSpace,
        queue: InputQueue,
        slack: u32,
    ) -> Self {
        let scale = objc2_app_kit::NSScreen::mainScreen(mtm)
            .map(|screen| screen.backingScaleFactor())
            .unwrap_or(1.0);
        let (pw, ph) = panel_size(128, 128, slack);
        let size = f64::from(pw.max(ph));
        let pool = super::panel_pool::PanelPool::prewarm(mtm, POOL_SIZE, size, &queue, scale);
        Self {
            mtm,
            live: Vec::new(),
            pool,
            cache,
            poses,
            space,
            slack,
            queue,
            scale,
            user_scale: 1.0,
            opacity: 1.0,
            filter_linear: false,
            keep_on_screen: true,
            layout_dirty: false,
            counters: CommitCounters::default(),
        }
    }

    pub fn set_user_scale(&mut self, scale: f64) {
        self.user_scale = scale;
        for panel in &mut self.live {
            let transform = sprite_transform(panel.last_mirrored, scale);
            panel.sprite.setTransform(transform);
        }
    }

    pub fn drop_headroom(&self) -> i32 {
        if self.keep_on_screen {
            self.poses.drop_headroom(self.user_scale)
        } else {
            0
        }
    }

    pub fn set_opacity(&mut self, opacity: f64) {
        self.opacity = opacity;
        for panel in &self.live {
            panel.window.setAlphaValue(opacity);
        }
    }

    pub fn set_filter_linear(&mut self, linear: bool) {
        self.filter_linear = linear;
        for panel in &self.live {
            panel.set_sampling(linear);
        }
    }

    fn sprite_transform_now(&self, mirror: bool) -> CATransform3D {
        sprite_transform(mirror, self.user_scale)
    }

    pub fn set_keep_on_screen(&mut self, on: bool) {
        if self.keep_on_screen == on {
            return;
        }
        self.keep_on_screen = on;
        self.layout_dirty = true;
    }

    pub fn set_space(&mut self, space: CocoaSpace) {
        if self.space == space {
            return;
        }
        self.space = space;
        self.layout_dirty = true;
    }

    fn work_rects(&self) -> Vec<Rect> {
        self.space
            .screens
            .iter()
            .map(|screen| work_area(screen, self.space.primary_height))
            .collect()
    }

    pub fn input(&self) -> &InputQueue {
        &self.queue
    }

    pub fn decoded_bytes(&self) -> u64 {
        self.cache.decoded_bytes()
    }

    pub fn live_ids(&self) -> Vec<MascotId> {
        self.live.iter().map(|panel| panel.mascot).collect()
    }

    /// Sprite rectangle in Cocoa coordinates, once the pet is on screen.
    pub fn sprite_cocoa(&self, id: MascotId) -> Option<Rect> {
        let panel = self.live.iter().find(|panel| panel.mascot == id)?;
        if !panel.shown {
            return None;
        }
        let sprite = panel.last_sprite?;
        Some(self.space.rect_to_cocoa(sprite))
    }

    pub fn trace_geometry(&self, id: MascotId) -> Option<String> {
        let panel = self.live.iter().find(|panel| panel.mascot == id)?;
        Some(format!(
            "panel_model={:?} panel_actual={:?} view_actual={:?} sprite_actual={:?}",
            panel.panel,
            panel.window.frame(),
            panel.view.frame(),
            panel.sprite.frame(),
        ))
    }

    fn pixel_hit(&self, panel: &Panel, point: Point) -> bool {
        if self.opacity == 0.0 || !panel.shown {
            return false;
        }
        let Some(rect) = panel.last_sprite else {
            return false;
        };
        panel.mask.as_ref().is_some_and(|mask| {
            mask.hit_in_cell(
                (
                    f64::from(point.x) - f64::from(rect.x),
                    f64::from(point.y) - f64::from(rect.y),
                ),
                (rect.width, rect.height),
                self.user_scale,
                panel.last_mirrored,
            )
        })
    }

    pub fn pointer_from_cocoa(&self, x: i32, y: i32) -> Point {
        self.space.point_to_engine(x, y)
    }

    /// Also called by local/global pointer monitors while the clock is stopped.
    pub fn refresh_mouse(&mut self, point: Point) {
        for index in 0..self.live.len() {
            let hit = self.pixel_hit(&self.live[index], point);
            let panel = &mut self.live[index];
            panel.view.set_pointer_hit(hit);
            // A grab retains its mouse-up even if the cursor leaves the art.
            let ignores = !hit && !panel.view.dragging();
            if panel.ignores_mouse != Some(ignores) {
                panel.window.setIgnoresMouseEvents(ignores);
                panel.ignores_mouse = Some(ignores);
            }
        }
    }

    fn take(&mut self, id: MascotId) -> Panel {
        let (pw, ph) = panel_size(128, 128, self.slack);
        let size = f64::from(pw.max(ph));
        let mut panel = self
            .pool
            .take(self.mtm, size, id, self.queue.clone(), self.scale);
        panel.mascot = id;
        panel.bind(id);
        panel.panel = None;
        panel.last_sprite = None;
        panel.last_image = None;
        panel.last_key = None;
        panel.last_look_right = false;
        panel.last_mirrored = false;
        panel.shown = false;
        panel.mask = None;
        panel.window.setAlphaValue(self.opacity);
        panel.set_sampling(self.filter_linear);
        panel
    }

    fn apply_one(&mut self, frame: &MascotFrame, force_move: bool, force_image: bool) {
        let pose = self.poses.lookup(frame.image.as_deref()).clone();
        let placed = placement_anchor(
            pose.anchor,
            pose.width,
            frame.look_right,
            pose.right_path.is_some(),
        );
        let origin = sprite_origin(frame.anchor, placed);
        let mut sprite = Rect::new(origin.x, origin.y, pose.width, pose.height);
        let mirror = frame.look_right && pose.right_path.is_none();
        let opaque = if mirror {
            pose.opaque.mirrored(pose.width)
        } else if frame.look_right {
            pose.right_path
                .as_deref()
                .and_then(|path| self.poses.image_span(path))
                .unwrap_or(pose.opaque)
        } else {
            pose.opaque
        };
        if self.keep_on_screen {
            let works = self.work_rects();
            sprite = clamp_sprite_x(sprite, frame.anchor, &works, opaque);
        }
        let (panel_w, panel_h) = panel_size(pose.width, pose.height, self.slack);
        let Some(index) = self.live.iter().position(|panel| panel.mascot == frame.id) else {
            return;
        };
        let delta = match self.live[index].last_sprite {
            Some(prev) => Point::new(sprite.x.wrapping_sub(prev.x), sprite.y.wrapping_sub(prev.y)),
            None => Point::new(0, 0),
        };
        let inside_actual = self.live[index].panel.is_some_and(|panel| {
            sprite_inside(
                Point::new(
                    sprite.x.wrapping_sub(panel.x),
                    sprite.y.wrapping_sub(panel.y),
                ),
                sprite.width,
                sprite.height,
                panel.width,
                panel.height,
            )
        });
        let next = if inside_actual && !force_move {
            self.live[index].panel.unwrap()
        } else {
            keep_sprite_inside(
                sprite,
                next_panel_origin(sprite, self.live[index].panel, panel_w, panel_h, delta),
            )
        };

        let image_key = if frame.look_right {
            pose.right_path
                .as_deref()
                .or(frame.image.as_deref())
                .map(str::to_string)
        } else {
            frame.image.clone()
        };
        let image_changed = force_image || image_key != self.live[index].last_key;

        if image_changed {
            self.live[index].mask = image_key.as_deref().and_then(|key| self.cache.mask(key));
            let contents = image_key.as_deref().and_then(|key| self.cache.as_id(key));
            unsafe { self.live[index].sprite.setContents(contents) };
            self.counters.contents_sets += 1;
            let transform = self.sprite_transform_now(mirror);
            self.live[index].sprite.setTransform(transform);
            self.live[index].set_sampling(self.filter_linear);
            self.live[index].last_key = image_key;
            self.live[index].last_image = frame.image.clone();
            self.live[index].last_look_right = frame.look_right;
            self.live[index].last_mirrored = mirror;
        } else if frame.look_right != self.live[index].last_look_right
            || mirror != self.live[index].last_mirrored
        {
            let transform = self.sprite_transform_now(mirror);
            self.live[index].sprite.setTransform(transform);
            self.live[index].last_look_right = frame.look_right;
            self.live[index].last_mirrored = mirror;
        }

        if self.live[index].panel != Some(next) {
            let cocoa = self.space.rect_to_cocoa(next);
            let origin = NSPoint {
                x: f64::from(cocoa.x),
                y: f64::from(cocoa.y),
            };
            let size = NSSize {
                width: f64::from(next.width),
                height: f64::from(next.height),
            };
            let current = self.live[index].window.frame().size;
            if panel_needs_resize((current.width, current.height), (size.width, size.height)) {
                self.live[index].window.setContentSize(size);
            }
            self.live[index].window.setFrameOrigin(origin);
            self.counters.window_moves += 1;
            self.live[index].panel = Some(next);
        }
        self.live[index].last_sprite = Some(sprite);

        let panel = self.live[index].panel.unwrap_or(next);
        let local_x = sprite.x.wrapping_sub(panel.x);
        let local_y = panel
            .height
            .wrapping_sub(sprite.y.wrapping_sub(panel.y).wrapping_add(sprite.height));
        set_layer_frame(
            &self.live[index].sprite,
            f64::from(local_x),
            f64::from(local_y),
            f64::from(sprite.width),
            f64::from(sprite.height),
        );
        self.counters.layer_moves += 1;

        if !self.live[index].shown {
            self.live[index].show();
            self.live[index].shown = true;
        }
    }
}

impl Renderer for PanelRenderer {
    fn attach(&mut self, id: MascotId, frame: &MascotFrame) {
        if self.live.iter().any(|panel| panel.mascot == id) {
            return;
        }
        let panel = self.take(id);
        self.live.push(panel);
        self.apply_one(frame, true, true);
    }

    fn detach(&mut self, id: MascotId) {
        if let Some(index) = self.live.iter().position(|panel| panel.mascot == id) {
            let mut panel = self.live.remove(index);
            panel.hide();
            panel.shown = false;
            self.pool.give(panel);
        }
    }

    fn commit(&mut self, frames: &[MascotFrame], deltas: &[MascotDelta]) {
        let force = self.layout_dirty;
        self.layout_dirty = false;
        for frame in frames {
            let delta = deltas.iter().find(|item| item.id == frame.id);
            let anchor_changed = delta.is_some_and(|item| item.anchor_changed);
            let image_changed = delta.is_some_and(|item| item.image_changed);
            if !force && !anchor_changed && !image_changed {
                continue;
            }
            self.apply_one(frame, false, image_changed);
        }
    }

    fn hit_test(&self, engine_point: Point) -> Option<MascotId> {
        self.live
            .iter()
            .rev()
            .find(|panel| self.pixel_hit(panel, engine_point))
            .map(|panel| panel.mascot)
    }

    fn show_all(&mut self) {
        for panel in &self.live {
            panel.show();
        }
    }

    fn hide_all(&mut self) {
        for panel in &self.live {
            panel.hide();
        }
    }
}

fn sprite_transform(mirror: bool, user_scale: f64) -> CATransform3D {
    let identity = (user_scale - 1.0).abs() < f64::EPSILON;
    if mirror && identity {
        CATransform3D::new_scale(-1.0, 1.0, 1.0)
    } else if !mirror && identity {
        unsafe { CATransform3DIdentity }
    } else if mirror {
        CATransform3D::new_scale(-user_scale, user_scale, 1.0)
    } else {
        CATransform3D::new_scale(user_scale, user_scale, 1.0)
    }
}

fn set_layer_frame(layer: &CALayer, x: f64, y: f64, w: f64, h: f64) {
    // setFrame on a transformed layer compensates its bounds for the scale.
    // Keep image bounds stable and place its centre explicitly instead.
    let bounds = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h));
    layer.setBounds(bounds);
    layer.setPosition(CGPoint::new(x + w / 2.0, y + h / 2.0));
}

fn disable_layer_actions(layer: &CALayer) {
    let Some(null_cls) = AnyClass::get(c"NSNull") else {
        return;
    };
    let null: Retained<AnyObject> = unsafe { msg_send![null_cls, null] };
    let keys = ["position", "bounds", "contents", "transform"];
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
