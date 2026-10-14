//! Keylens' macOS platform layer.
//!
//! `convert`, `menu_tree` and `snapshots` are pure logic, compiled and tested on all platforms;
//! the remaining modules call macOS system APIs and are compiled only on macOS.

pub mod convert;
pub mod menu_tree;
pub mod snapshots;

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod exec;
#[cfg(target_os = "macos")]
mod keytap;
#[cfg(target_os = "macos")]
mod menu;
#[cfg(target_os = "macos")]
pub mod panel;
#[cfg(target_os = "macos")]
mod platform;
#[cfg(target_os = "macos")]
pub mod tray;
#[cfg(target_os = "macos")]
mod workspace;

#[cfg(target_os = "macos")]
pub use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
pub use platform::MacPlatform;

/// Helper functions for development and debugging; not part of the stable interface.
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub mod debug {
    pub use crate::ax::window_titles;
}
