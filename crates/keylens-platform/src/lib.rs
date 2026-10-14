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

/// 按键监听的生命周期。drop 时停止监听。
pub struct KeyEventGuard(#[allow(dead_code)] Box<dyn Any + Send>);

impl KeyEventGuard {
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
    fn execute(&self, entry: &Entry) -> Result<(), PlatformError>;
    /// 开始把全局按键事件发送到 `tx`，直到返回的 guard 被 drop。
    fn start_key_events(&self, tx: Sender<KeyEvent>) -> Result<KeyEventGuard, PlatformError>;
    fn default_trigger(&self) -> Trigger;
}
