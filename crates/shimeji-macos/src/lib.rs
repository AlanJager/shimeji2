//! macOS shell. Platform code is behind `cfg(target_os = "macos")`.
//! Coordinate conversion and window-list selection are pure logic.

pub mod bundle_pack;
pub mod launch;
pub mod mischief;
pub mod platform;
pub mod threads;
pub mod updates;

#[cfg(target_os = "macos")]
pub mod agent;
#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod bridge;
#[cfg(target_os = "macos")]
mod ui;

/// Crate name. Dependents call this so the workspace link is real.
pub fn crate_name() -> &'static str {
    let _app = shimeji_app::crate_name();
    "shimeji-macos"
}

/// Entry used by the binary. On other OSes the process prints and exits 2.
pub fn run() -> std::process::ExitCode {
    #[cfg(target_os = "macos")]
    {
        std::process::ExitCode::from(app::run())
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("shimeji-macos is a macOS app");
        std::process::ExitCode::from(2)
    }
}
