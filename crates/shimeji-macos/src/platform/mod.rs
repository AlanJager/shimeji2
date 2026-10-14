//! Platform adapters. Pure-logic modules always compile; AppKit stays on macOS.

pub mod context_menu;
pub mod coords;
pub mod counters;
pub mod hit_test;
pub mod hotkey;
pub mod poses;
pub mod power;
pub mod render;
pub mod slack;
pub mod sound;
pub mod spawn;
pub mod window_env;

#[cfg(target_os = "macos")]
pub mod clock;
#[cfg(target_os = "macos")]
pub mod image_cache;
#[cfg(target_os = "macos")]
pub mod input;
#[cfg(target_os = "macos")]
pub mod panel;
#[cfg(target_os = "macos")]
pub mod panel_pool;
#[cfg(target_os = "macos")]
pub mod pointer;
#[cfg(target_os = "macos")]
pub mod screens;
