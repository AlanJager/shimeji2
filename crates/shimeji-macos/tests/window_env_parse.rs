//! CGWindowList record selection. No WindowServer.

use shimeji_macos::platform::coords::Rect;
use shimeji_macos::platform::window_env::{
    SelectInput, WindowRecord, select_active_ie, to_window_sample,
};

fn rec(id: u64, pid: i32, layer: i32, alpha: f64, bounds: Rect) -> WindowRecord {
    WindowRecord {
        id,
        owner_pid: pid,
        layer,
        alpha,
        bounds,
    }
}

fn screen() -> Rect {
    Rect::new(0, 0, 1920, 1080)
}

#[test]
fn frontmost_pid_gives_the_first_layer_zero_window_as_active_ie() {
    let windows = [
        rec(11, 7, 25, 1.0, Rect::new(10, 10, 20, 20)), // menu, layer > 0
        rec(12, 7, 0, 1.0, Rect::new(100, 80, 640, 400)),
        rec(13, 7, 0, 1.0, Rect::new(200, 90, 320, 200)),
        rec(14, 9, 0, 1.0, Rect::new(0, 0, 100, 100)),
    ];
    let active = select_active_ie(SelectInput {
        windows: &windows,
        frontmost_pid: 7,
        our_pid: 42,
        screens: &[screen()],
    })
    .expect("active window");
    assert_eq!(active.sample.id, 12);
    assert_eq!(active.sample.x, 100);
    assert_eq!(active.sample.y, 80);
    assert_eq!(active.sample.width, 640);
    assert_eq!(active.sample.height, 400);
    assert!(active.sample.visible);
    assert!(active.top_standable);
}

#[test]
fn our_windows_layer_above_zero_and_alpha_zero_are_ignored() {
    let windows = [
        rec(1, 42, 0, 1.0, Rect::new(0, 0, 128, 128)), // ours
        rec(2, 7, 3, 1.0, Rect::new(10, 10, 50, 50)),  // layer > 0
        rec(3, 7, 0, 0.0, Rect::new(20, 20, 50, 50)),  // alpha 0
        rec(4, 7, 0, 1.0, Rect::new(300, 200, 400, 300)),
    ];
    let active = select_active_ie(SelectInput {
        windows: &windows,
        frontmost_pid: 7,
        our_pid: 42,
        screens: &[screen()],
    })
    .expect("active window");
    assert_eq!(active.sample.id, 4);
    assert_eq!(active.sample.x, 300);
    assert!(active.top_standable);
}

#[test]
fn full_screen_window_top_border_is_not_standable() {
    let windows = [rec(9, 7, 0, 1.0, screen())];
    let active = select_active_ie(SelectInput {
        windows: &windows,
        frontmost_pid: 7,
        our_pid: 42,
        screens: &[screen()],
    })
    .expect("active window");
    assert_eq!(active.sample.id, 9);
    assert_eq!(active.sample.width, 1920);
    assert_eq!(active.sample.height, 1080);
    assert!(
        !active.top_standable,
        "full-screen top is not a standable floor"
    );
    let sample = to_window_sample(&active);
    assert!(
        !sample.visible,
        "snapshot clears isOn so the top is not a floor"
    );
    assert_eq!(sample.id, 9);
    assert_eq!(sample.width, 1920);
}
