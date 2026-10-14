//! 测试用的平台实现：所有状态都在内存里，可以从测试代码直接控制。

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use keylens_core::{
    AppInfo, Entry, EntryId, KeyEvent, Modifiers, ShortcutSource, SourceError, Trigger,
};

use crate::{KeyEventGuard, PermissionState, Platform, PlatformError};

type KeySender = Arc<Mutex<Option<Sender<KeyEvent>>>>;

pub struct FakePlatform {
    permission: Mutex<PermissionState>,
    app: Mutex<Option<AppInfo>>,
    menu: Arc<Mutex<Vec<Entry>>>,
    executed: Mutex<Vec<EntryId>>,
    key_sender: KeySender,
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self {
            permission: Mutex::new(PermissionState::Granted),
            app: Mutex::new(None),
            menu: Arc::default(),
            executed: Mutex::default(),
            key_sender: Arc::default(),
        }
    }
}

impl FakePlatform {
    pub fn set_permission(&self, state: PermissionState) {
        *self.permission.lock().unwrap() = state;
    }

    pub fn set_frontmost(&self, app: Option<AppInfo>) {
        *self.app.lock().unwrap() = app;
    }

    pub fn set_menu(&self, entries: Vec<Entry>) {
        *self.menu.lock().unwrap() = entries;
    }

    pub fn executed(&self) -> Vec<EntryId> {
        self.executed.lock().unwrap().clone()
    }

    /// 模拟一次全局按键事件。没有在监听时什么都不做。
    pub fn send_key(&self, event: KeyEvent) {
        if let Some(tx) = self.key_sender.lock().unwrap().as_ref() {
            let _ = tx.send(event);
        }
    }
}

struct FakeMenu(Arc<Mutex<Vec<Entry>>>);

impl ShortcutSource for FakeMenu {
    fn load(&self, _: &AppInfo) -> Result<Vec<Entry>, SourceError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

/// drop 时清空发送端，停止转发事件。
struct StopOnDrop(KeySender);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.lock().unwrap().take();
    }
}

impl Platform for FakePlatform {
    fn permission(&self) -> PermissionState {
        *self.permission.lock().unwrap()
    }

    fn request_permission(&self) {}

    fn frontmost_app(&self) -> Option<AppInfo> {
        self.app.lock().unwrap().clone()
    }

    fn menu_source(&self) -> Box<dyn ShortcutSource> {
        Box::new(FakeMenu(self.menu.clone()))
    }

    fn execute(&self, entry: &Entry) -> Result<(), PlatformError> {
        if !entry.enabled {
            return Err(PlatformError::Disabled);
        }
        self.executed.lock().unwrap().push(entry.id);
        Ok(())
    }

    fn start_key_events(&self, tx: Sender<KeyEvent>) -> Result<KeyEventGuard, PlatformError> {
        if self.permission() == PermissionState::Denied {
            return Err(PlatformError::PermissionDenied);
        }
        *self.key_sender.lock().unwrap() = Some(tx);
        Ok(KeyEventGuard::new(StopOnDrop(self.key_sender.clone())))
    }

    fn default_trigger(&self) -> Trigger {
        Trigger::HoldModifier {
            key: Modifiers::META,
            delay: Trigger::DEFAULT_HOLD_DELAY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::{EntrySource, Modifiers, Os, Shortcut};

    fn entry(index: u32, enabled: bool) -> Entry {
        Entry {
            id: EntryId {
                source: EntrySource::Menu,
                index,
            },
            group: "文件".into(),
            path: vec![],
            title: "保存".into(),
            search_terms: vec![],
            shortcut: Shortcut::parse_seq(&["Mod+S"], Os::MacOS).unwrap(),
            enabled,
        }
    }

    #[test]
    fn records_executed_entries_and_refuses_disabled_ones() {
        let platform = FakePlatform::default();
        platform.execute(&entry(0, true)).unwrap();
        assert!(matches!(
            platform.execute(&entry(1, false)),
            Err(PlatformError::Disabled)
        ));
        assert_eq!(
            platform.executed(),
            [EntryId {
                source: EntrySource::Menu,
                index: 0
            }]
        );
    }

    #[test]
    fn menu_source_returns_configured_entries() {
        let platform = FakePlatform::default();
        platform.set_menu(vec![entry(0, true)]);
        let app = AppInfo {
            pid: 1,
            name: "Demo".into(),
            ids: vec![],
        };
        assert_eq!(platform.menu_source().load(&app).unwrap().len(), 1);
    }

    #[test]
    fn key_events_flow_until_the_guard_is_dropped() {
        let platform = FakePlatform::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let guard = platform.start_key_events(tx).unwrap();
        platform.send_key(KeyEvent::ModifiersChanged(Modifiers::META));
        assert_eq!(
            rx.try_recv().unwrap(),
            KeyEvent::ModifiersChanged(Modifiers::META)
        );
        drop(guard);
        platform.send_key(KeyEvent::KeyDown);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn permission_can_be_toggled() {
        let platform = FakePlatform::default();
        assert_eq!(platform.permission(), PermissionState::Granted);
        platform.set_permission(PermissionState::Denied);
        assert_eq!(platform.permission(), PermissionState::Denied);
        assert!(matches!(
            platform.start_key_events(std::sync::mpsc::channel().0),
            Err(PlatformError::PermissionDenied)
        ));
    }
}
