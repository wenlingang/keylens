//! Keylens 的 macOS 平台层。
//!
//! `convert`、`menu_tree`、`snapshots` 是纯逻辑，在所有平台编译并测试；
//! 其余模块调用 macOS 系统 API，只在 macOS 上编译。

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

/// 开发调试用的辅助函数，不属于稳定接口。
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub mod debug {
    pub use crate::ax::window_titles;
}
