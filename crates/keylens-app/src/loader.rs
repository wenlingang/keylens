//! Combines the menu, user data and built-in data into one app's shortcuts, and caches results per app.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

use keylens_core::community::{Catalog, CommunitySource};
use keylens_core::{AppInfo, AppShortcuts, EntrySource, Lang, Os, ShortcutSource, collect};
use keylens_platform::Platform;

pub struct Loader {
    platform: Arc<dyn Platform>,
    builtin: CommunitySource,
    /// Replaced when the user edits files in the user data folder.
    user: RwLock<CommunitySource>,
    os: Os,
    lang: Lang,
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
            user: RwLock::new(CommunitySource::new(user, os, lang, EntrySource::User)),
            os,
            lang,
        }
    }

    /// Replaces the user data (after the user saved a file); later loads use it.
    pub fn set_user_catalog(&self, catalog: Arc<Catalog>) {
        let source = CommunitySource::new(catalog, self.os, self.lang, EntrySource::User);
        *self.user.write().unwrap() = source;
    }

    /// Reads one app's shortcuts. Blocks (reading the menu takes up to ~2 s), so call it from a background thread.
    pub fn load(&self, app: &AppInfo) -> AppShortcuts {
        let menu = self.platform.menu_source();
        let user = self.user.read().unwrap();
        let sources: [&dyn ShortcutSource; 3] = [menu.as_ref(), &*user, &self.builtin];
        collect(app.clone(), &sources)
    }
}

/// Decides whether a finished read should replace what is shown and cached.
///
/// Several reads of the same app can be in flight (app switch, ⌘ press, panel shown) and may
/// finish out of order. The platform keeps only the menu snapshot with the highest id, so the
/// UI must too: otherwise every menu entry it shows is stale and executing it only beeps.
/// A read whose menu failed (e.g. timed out) never replaces good menu data.
#[derive(Default)]
pub struct Freshness {
    applied: HashMap<i32, u64>,
}

impl Freshness {
    pub fn accept(&mut self, data: &AppShortcuts) -> bool {
        let pid = data.app.pid;
        let snapshot = data
            .groups
            .iter()
            .flat_map(|g| &g.entries)
            .filter(|e| e.source() == EntrySource::Menu)
            .map(|e| e.id.snapshot)
            .max();
        match (snapshot, self.applied.get(&pid).copied()) {
            (Some(new), Some(old)) if new <= old => false,
            (Some(new), _) => {
                self.applied.insert(pid, new);
                true
            }
            // The menu was read successfully but has no shortcuts: take it.
            (None, _) if data.warnings.is_empty() => {
                self.applied.remove(&pid);
                true
            }
            // The menu read failed: only use it if there is no good menu data to keep.
            (None, old) => old.is_none(),
        }
    }
}

/// Most recently read app shortcuts, keyed by pid.
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

    fn read(pid: i32, snapshot: Option<u64>) -> AppShortcuts {
        let mut entry = menu_entry();
        let groups = match snapshot {
            Some(id) => {
                entry.id.snapshot = id;
                vec![keylens_core::Group {
                    title: "文件".into(),
                    community: false,
                    entries: vec![entry],
                }]
            }
            None => vec![],
        };
        let warnings = if snapshot.is_some() {
            vec![]
        } else {
            vec![SourceError::Timeout]
        };
        AppShortcuts {
            app: app(pid),
            groups,
            warnings,
        }
    }

    #[test]
    fn an_older_read_arriving_late_is_ignored() {
        let mut freshness = Freshness::default();
        assert!(freshness.accept(&read(1, Some(6))));
        assert!(!freshness.accept(&read(1, Some(5))));
        assert!(freshness.accept(&read(1, Some(7))));
        // Other apps are tracked separately.
        assert!(freshness.accept(&read(2, Some(3))));
    }

    #[test]
    fn a_failed_read_does_not_replace_good_menu_data() {
        let mut freshness = Freshness::default();
        // Nothing good yet: showing community data with a warning is the best we have.
        assert!(freshness.accept(&read(1, None)));
        assert!(freshness.accept(&read(1, Some(4))));
        assert!(!freshness.accept(&read(1, None)));
        assert!(freshness.accept(&read(1, Some(9))));
    }

    #[test]
    fn apps_without_menu_shortcuts_are_always_refreshed() {
        let mut freshness = Freshness::default();
        let mut empty = read(1, None);
        empty.warnings.clear();
        assert!(freshness.accept(&empty));
        assert!(freshness.accept(&empty));
    }

    #[test]
    fn user_data_can_be_replaced_while_running() {
        let platform = Arc::new(FakePlatform::default());
        let l = loader(platform);
        let titles = |data: &AppShortcuts| -> Vec<String> {
            data.groups
                .iter()
                .flat_map(|g| g.entries.iter().map(|e| e.title.clone()))
                .collect()
        };
        assert!(!titles(&l.load(&app(7))).contains(&"My Shortcut".to_string()));
        let mine = "schema = 1\n[app]\nname = \"Demo\"\n[app.match]\nmacos = [\"com.demo\"]\n\
                    [[group]]\ntitle = \"Mine\"\n[[group.shortcut]]\ntitle = \"My Shortcut\"\nkeys = \"Mod+J\"\n";
        let (user, errors) = Catalog::from_files([("demo.toml", mine)]);
        assert!(errors.is_empty());
        l.set_user_catalog(Arc::new(user));
        assert!(titles(&l.load(&app(7))).contains(&"My Shortcut".to_string()));
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
