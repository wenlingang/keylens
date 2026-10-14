//! Keylens 的 macOS 平台层。
//!
//! `convert`、`menu_tree`、`snapshots` 是纯逻辑，在所有平台编译并测试；
//! 其余模块调用 macOS 系统 API，只在 macOS 上编译。

pub mod convert;
pub mod menu_tree;
pub mod snapshots;
