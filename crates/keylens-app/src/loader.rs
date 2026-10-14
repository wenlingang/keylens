//! 把菜单、用户数据和内置数据合成一个应用的快捷键，以及按应用缓存结果。

use std::collections::VecDeque;
use std::sync::Arc;

use keylens_core::community::{Catalog, CommunitySource};
use keylens_core::{AppInfo, AppShortcuts, EntrySource, Lang, Os, ShortcutSource, collect};
use keylens_platform::Platform;

pub struct Loader {
    platform: Arc<dyn Platform>,
    builtin: CommunitySource,
    user: CommunitySource,
}

impl Loader {
    pub fn new(
        platform: Arc<dyn Platform>,
        builtin: Arc<Catalog>,
        user: Arc<Catalog>,
        os: Os,
        lang: Lang,
    ) -> Self {
        Self {
            platform,
            builtin: CommunitySource::new(builtin, os, lang, EntrySource::Community),
            user: CommunitySource::new(user, os, lang, EntrySource::User),
        }
    }

    /// 读取一个应用的快捷键。会阻塞（读菜单最多约 2 秒），要在后台线程调用。
    pub fn load(&self, app: &AppInfo) -> AppShortcuts {
        let menu = self.platform.menu_source();
        let sources: [&dyn ShortcutSource; 3] = [menu.as_ref(), &self.user, &self.builtin];
        collect(app.clone(), &sources)
    }
}

/// 最近读取过的应用快捷键，按 pid 保存。
pub struct Cache {
    capacity: usize,
    items: VecDeque<AppShortcuts>,
}

impl Cache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: VecDeque::new(),
        }
    }

    pub fn get(&self, pid: i32) -> Option<&AppShortcuts> {
        self.items.iter().find(|s| s.app.pid == pid)
    }

    pub fn put(&mut self, shortcuts: AppShortcuts) {
        self.items.retain(|s| s.app.pid != shortcuts.app.pid);
        self.items.push_back(shortcuts);
        while self.items.len() > self.capacity {
            self.items.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::community::Catalog;
    use keylens_core::{Entry, EntryId, Shortcut, SourceError};
    use keylens_platform::PermissionState;
    use keylens_platform::fake::FakePlatform;

    const DATA: &str = r#"
schema = 1
[app]
name = "Demo"
[app.match]
macos = ["com.demo"]
[[group]]
title = "General"
i18n = { zh-Hans = "常规" }
[[group.shortcut]]
title = "Save"
i18n = { zh-Hans = "保存" }
keys = "Mod+S"
[[group.shortcut]]
title = "Zen Mode"
keys = ["Mod+K", "Z"]
"#;

    fn app(pid: i32) -> AppInfo {
        AppInfo {
            pid,
            name: "Demo".into(),
            ids: vec!["com.demo".into()],
        }
    }

    fn menu_entry() -> Entry {
        Entry {
            id: EntryId {
                source: EntrySource::Menu,
                snapshot: 0,
                index: 0,
            },
            group: "文件".into(),
            path: vec![],
            title: "存储".into(),
            search_terms: vec![],
            shortcut: Shortcut::parse_seq(&["Mod+S"], Os::MacOS).unwrap(),
            enabled: true,
        }
    }

    fn loader(platform: Arc<FakePlatform>) -> Loader {
        let (builtin, errors) = Catalog::from_files([("demo.toml", DATA)]);
        assert!(errors.is_empty());
        let (user, _) = Catalog::from_files(Vec::<(String, String)>::new());
        Loader::new(
            platform,
            Arc::new(builtin),
            Arc::new(user),
            Os::MacOS,
            Lang::ZhHans,
        )
    }

    #[test]
    fn merges_menu_and_community_with_menu_winning() {
        let platform = Arc::new(FakePlatform::default());
        platform.set_menu(vec![menu_entry()]);
        let result = loader(platform).load(&app(7));
        let titles: Vec<_> = result
            .groups
            .iter()
            .flat_map(|g| g.entries.iter().map(|e| e.title.as_str()))
            .collect();
        assert_eq!(titles, ["存储", "Zen Mode"]);
        assert!(result.groups[1].community);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn menu_failure_still_returns_community_data_with_a_warning() {
        let platform = Arc::new(FakePlatform::default());
        platform.set_permission(PermissionState::Denied);
        platform.set_menu_error(Some(SourceError::Timeout));
        let result = loader(platform).load(&app(7));
        assert_eq!(result.warnings, [SourceError::Timeout]);
        assert_eq!(result.groups.len(), 1);
    }

    #[test]
    fn cache_keeps_latest_per_app_and_evicts_oldest() {
        let platform = Arc::new(FakePlatform::default());
        let l = loader(platform);
        let mut cache = Cache::new(2);
        cache.put(l.load(&app(1)));
        cache.put(l.load(&app(2)));
        cache.put(l.load(&app(1)));
        cache.put(l.load(&app(3)));
        assert!(cache.get(2).is_none());
        assert!(cache.get(1).is_some());
        assert!(cache.get(3).is_some());
    }
}
