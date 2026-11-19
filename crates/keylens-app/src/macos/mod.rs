//! The macOS side: Accessibility API (menu reads and presses), the key listener, the panel
//! window, the menu bar item and installing updates.
//!
//! `convert`, `geometry`, `menu_tree`, `snapshots` and `synthetic` are pure logic with unit tests;
//! the remaining modules call macOS system APIs.

pub mod convert;
pub mod geometry;
pub mod menu_tree;
pub mod snapshots;
pub mod synthetic;

mod ax;
mod exec;
mod keytap;
mod menu;
pub mod panel;
mod platform;
pub mod tray;
pub mod update;
mod workspace;

pub use platform::MacPlatform;

/// Helper functions for development and debugging; not part of the stable interface.
#[doc(hidden)]
pub mod debug {
    pub use crate::macos::ax::window_titles;
}
