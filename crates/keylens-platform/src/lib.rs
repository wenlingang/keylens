//! 平台层接口。macOS 实现在 `keylens-macos`，测试用 `fake::FakePlatform`。

use std::any::Any;
use std::sync::mpsc::Sender;

use keylens_core::{AppInfo, Entry, KeyEvent, ShortcutSource, Trigger};

#[cfg(any(test, feature = "fake"))]
pub mod fake;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("accessibility permission is not granted")]
    PermissionDenied,
    #[error("the menu item no longer exists")]
    Stale,
    #[error("the menu item is disabled")]
    Disabled,
    #[error("{0}")]
    Other(String),
}

/// 一个监听（按键事件、前台应用切换）的生命周期。drop 时停止监听。
pub struct ListenGuard(#[allow(dead_code)] Box<dyn Any + Send>);

impl ListenGuard {
    pub fn new(inner: impl Any + Send) -> Self {
        Self(Box::new(inner))
    }
}

pub trait Platform: Send + Sync {
    fn permission(&self) -> PermissionState;
    /// 打开系统的授权界面。
    fn request_permission(&self);
    fn frontmost_app(&self) -> Option<AppInfo>;
    /// 读取前台应用菜单的数据来源。
    fn menu_source(&self) -> Box<dyn ShortcutSource>;
    /// 执行一个条目：菜单条目点按菜单项，社区条目模拟按键。
    /// 菜单条目来自更早的一次读取（`EntryId::snapshot` 不是最新）时，
    /// 必须返回 `PlatformError::Stale`，或按分组、路径、标题重新确认后再执行，
    /// 绝不能按 `index` 直接执行，否则可能点到别的菜单项。
    fn execute(&self, entry: &Entry) -> Result<(), PlatformError>;
    /// 开始把全局按键事件发送到 `tx`，直到返回的 guard 被 drop。
    fn start_key_events(&self, tx: Sender<KeyEvent>) -> Result<ListenGuard, PlatformError>;
    /// 前台应用切换时把新应用发送到 `tx`，直到返回的 guard 被 drop。
    /// 界面层借此提前预读菜单：Electron 应用第一次读取约 900ms，超过长按时间。
    fn watch_frontmost(&self, tx: Sender<AppInfo>) -> Result<ListenGuard, PlatformError>;
    /// 应用图标（PNG 编码）。拿不到时返回 `None`，界面显示占位图标。
    fn app_icon(&self, app: &AppInfo) -> Option<Vec<u8>>;
    fn default_trigger(&self) -> Trigger;
}
