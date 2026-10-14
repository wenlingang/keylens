//! 测试用的平台实现：所有状态都在内存里，可以从测试代码直接控制。

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use keylens_core::{
    AppInfo, Entry, EntryId, EntrySource, KeyEvent, Modifiers, ShortcutSource, SourceError, Trigger,
};

use crate::{ListenGuard, PermissionState, Platform, PlatformError};

type SharedSender<T> = Arc<Mutex<Option<Sender<T>>>>;

/// 当前菜单和它的快照编号；每次 `set_menu` 编号加一，模拟一次新的菜单读取。
#[derive(Default)]
struct Menu {
    snapshot: u64,
    entries: Vec<Entry>,
}

pub struct FakePlatform {
    permission: Mutex<PermissionState>,
    app: Mutex<Option<AppInfo>>,
    menu: Arc<Mutex<Menu>>,
    executed: Mutex<Vec<EntryId>>,
    key_sender: SharedSender<KeyEvent>,
    app_sender: SharedSender<AppInfo>,
    icon: Mutex<Option<Vec<u8>>>,
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self {
            permission: Mutex::new(PermissionState::Granted),
            app: Mutex::new(None),
            menu: Arc::default(),
            executed: Mutex::default(),
            key_sender: Arc::default(),
            app_sender: Arc::default(),
            icon: Mutex::default(),
        }
    }
}

impl FakePlatform {
    pub fn set_permission(&self, state: PermissionState) {
        *self.permission.lock().unwrap() = state;
    }

    /// 切换前台应用；正在监听时会把新应用发送出去。
    pub fn set_frontmost(&self, app: Option<AppInfo>) {
        if let (Some(app), Some(tx)) = (&app, self.app_sender.lock().unwrap().as_ref()) {
            let _ = tx.send(app.clone());
        }
        *self.app.lock().unwrap() = app;
    }

    pub fn set_icon(&self, png: Option<Vec<u8>>) {
        *self.icon.lock().unwrap() = png;
    }

    pub fn set_menu(&self, entries: Vec<Entry>) {
        let mut menu = self.menu.lock().unwrap();
        menu.snapshot += 1;
        menu.entries = entries;
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

struct FakeMenu(Arc<Mutex<Menu>>);

impl ShortcutSource for FakeMenu {
    fn load(&self, _: &AppInfo) -> Result<Vec<Entry>, SourceError> {
        let menu = self.0.lock().unwrap();
        Ok(menu
            .entries
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.id.snapshot = menu.snapshot;
                entry
            })
            .collect())
    }
}

/// drop 时清空发送端，停止转发事件。
struct StopOnDrop<T>(SharedSender<T>);

impl<T> Drop for StopOnDrop<T> {
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
        let current = self.menu.lock().unwrap().snapshot;
        if entry.source() == EntrySource::Menu && entry.id.snapshot != current {
            return Err(PlatformError::Stale);
        }
        self.executed.lock().unwrap().push(entry.id);
        Ok(())
    }

    fn start_key_events(&self, tx: Sender<KeyEvent>) -> Result<ListenGuard, PlatformError> {
        if self.permission() == PermissionState::Denied {
            return Err(PlatformError::PermissionDenied);
        }
        *self.key_sender.lock().unwrap() = Some(tx);
        Ok(ListenGuard::new(StopOnDrop(self.key_sender.clone())))
    }

    fn watch_frontmost(&self, tx: Sender<AppInfo>) -> Result<ListenGuard, PlatformError> {
        *self.app_sender.lock().unwrap() = Some(tx);
        Ok(ListenGuard::new(StopOnDrop(self.app_sender.clone())))
    }

    fn app_icon(&self, _: &AppInfo) -> Option<Vec<u8>> {
        self.icon.lock().unwrap().clone()
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
    use keylens_core::{Modifiers, Os, Shortcut};

    fn entry(index: u32, enabled: bool) -> Entry {
        Entry {
            id: EntryId {
                source: EntrySource::Menu,
                snapshot: 0,
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
                snapshot: 0,
                index: 0
            }]
        );
    }

    #[test]
    fn entries_from_an_older_menu_snapshot_are_stale() {
        let platform = FakePlatform::default();
        let app = AppInfo {
            pid: 1,
            name: "Demo".into(),
            ids: vec![],
        };
        platform.set_menu(vec![entry(0, true)]);
        let old = platform.menu_source().load(&app).unwrap();
        platform.set_menu(vec![entry(0, true)]);
        let new = platform.menu_source().load(&app).unwrap();
        assert_ne!(old[0].id, new[0].id);
        assert!(matches!(
            platform.execute(&old[0]),
            Err(PlatformError::Stale)
        ));
        platform.execute(&new[0]).unwrap();
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
    fn frontmost_changes_are_reported_until_the_guard_is_dropped() {
        let platform = FakePlatform::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let guard = platform.watch_frontmost(tx).unwrap();
        let app = AppInfo {
            pid: 9,
            name: "Slack".into(),
            ids: vec!["com.tinyspeck.slackmacgap".into()],
        };
        platform.set_frontmost(Some(app.clone()));
        assert_eq!(rx.try_recv().unwrap(), app);
        assert_eq!(platform.frontmost_app(), Some(app.clone()));
        drop(guard);
        platform.set_frontmost(Some(app));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn app_icon_returns_configured_png() {
        let platform = FakePlatform::default();
        let app = AppInfo {
            pid: 9,
            name: "Slack".into(),
            ids: vec![],
        };
        assert_eq!(platform.app_icon(&app), None);
        platform.set_icon(Some(vec![0x89, b'P', b'N', b'G']));
        assert_eq!(platform.app_icon(&app), Some(vec![0x89, b'P', b'N', b'G']));
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
