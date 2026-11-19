//! Combines the menu, user data and built-in data into one app's shortcuts, and caches results per app.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use keylens_core::community::{Catalog, CommunitySource};
use keylens_core::{AppInfo, AppShortcuts, EntrySource, Lang, Os, ShortcutSource, collect};
use keylens_platform::Platform;

pub struct Loader {
    platform: Arc<dyn Platform>,
    os: Os,
    /// Rebuilt when the user edits their data files or switches the UI language.
    sources: RwLock<Sources>,
}

struct Sources {
    lang: Lang,
    builtin_catalog: Arc<Catalog>,
    user_catalog: Arc<Catalog>,
    builtin: Arc<CommunitySource>,
    user: Arc<CommunitySource>,
}

impl Sources {
    fn new(builtin_catalog: Arc<Catalog>, user_catalog: Arc<Catalog>, os: Os, lang: Lang) -> Self {
        Self {
            builtin: Arc::new(CommunitySource::new(
                builtin_catalog.clone(),
                os,
                lang,
                EntrySource::Community,
            )),
            user: Arc::new(CommunitySource::new(
                user_catalog.clone(),
                os,
                lang,
                EntrySource::User,
            )),
            lang,
            builtin_catalog,
            user_catalog,
        }
    }
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
            os,
            sources: RwLock::new(Sources::new(builtin, user, os, lang)),
        }
    }

    /// Replaces the user data (after the user saved a file); later loads use it.
    pub fn set_user_catalog(&self, catalog: Arc<Catalog>) {
        let mut sources = self.sources.write().unwrap();
        *sources = Sources::new(
            sources.builtin_catalog.clone(),
            catalog,
            self.os,
            sources.lang,
        );
    }

    /// Switches the language of community titles; later loads use it.
    pub fn set_lang(&self, lang: Lang) {
        let mut sources = self.sources.write().unwrap();
        *sources = Sources::new(
            sources.builtin_catalog.clone(),
            sources.user_catalog.clone(),
            self.os,
            lang,
        );
    }

    pub fn lang(&self) -> Lang {
        self.sources.read().unwrap().lang
    }

    /// Reads one app's shortcuts. Blocks (reading the menu takes up to ~2 s), so call it from a background thread.
    pub fn load(&self, app: &AppInfo) -> AppShortcuts {
        let menu = self.platform.menu_source();
        // Clone the sources and release the lock before reading the menu (up to ~2 s), so
        // `set_lang` / `set_user_catalog` on the UI thread never wait for it.
        let (user, builtin) = {
            let sources = self.sources.read().unwrap();
            (Arc::clone(&sources.user), Arc::clone(&sources.builtin))
        };
        let list: [&dyn ShortcutSource; 3] = [menu.as_ref(), user.as_ref(), builtin.as_ref()];
        collect(app.clone(), &list)
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

/// Decides whether to start reading an app's menu, so one press of the trigger key does not read
/// it twice (on the press, then again when the panel shows) and ordinary shortcuts such as ⌘C do
/// not read it every time.
///
/// A read is skipped while one for the same app is still running, or when one finished
/// successfully less than [`Reads::FRESH`] ago. Failed reads (e.g. timed out) never count as fresh.
#[derive(Default)]
pub struct Reads {
    /// When each running read started.
    running: HashMap<i32, Instant>,
    finished: HashMap<i32, Instant>,
}

impl Reads {
    /// How long a successful read stays fresh enough to show without reading again.
    pub const FRESH: Duration = Duration::from_secs(3);
    /// A read still marked running after this long is assumed lost (a menu read gives up after
    /// 2s), so a lost read cannot block the app's reads forever.
    const LOST: Duration = Duration::from_secs(10);

    /// Returns whether to read `pid` now, and if so marks the read as running.
    pub fn start(&mut self, pid: i32, now: Instant) -> bool {
        if let Some(&at) = self.running.get(&pid)
            && now.saturating_duration_since(at) < Self::LOST
        {
            return false;
        }
        if let Some(&at) = self.finished.get(&pid)
            && now.saturating_duration_since(at) < Self::FRESH
        {
            return false;
        }
        self.running.insert(pid, now);
        true
    }

    pub fn finish(&mut self, pid: i32, ok: bool, now: Instant) {
        self.running.remove(&pid);
        if ok {
            self.finished.insert(pid, now);
        } else {
            self.finished.remove(&pid);
        }
        // Only recent reads matter; drop the rest so quit apps do not pile up.
        self.finished
            .retain(|_, at| now.saturating_duration_since(*at) < Self::FRESH);
    }

    /// Forgets which reads are fresh, e.g. after the user data or language changed: the next
    /// request reads again.
    pub fn invalidate(&mut self) {
        self.running.clear();
        self.finished.clear();
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

    #[test]
    fn switching_language_changes_community_titles() {
        let platform = Arc::new(FakePlatform::default());
        platform.set_menu(vec![]);
        let text = r#"
schema = 1
[app]
name = "Demo"
[app.match]
macos = ["com.demo"]
[[group]]
title = "General"
i18n = { ja = "一般" }
  [[group.shortcut]]
  title = "Open"
  i18n = { ja = "開く" }
  keys = "Mod+O"
"#;
        let (catalog, errors) = Catalog::from_files([("demo.toml", text)]);
        assert!(errors.is_empty(), "{errors:?}");
        let loader = Loader::new(
            platform,
            Arc::new(catalog),
            Arc::new(Catalog::default()),
            Os::MacOS,
            Lang::En,
        );
        let app = AppInfo {
            pid: 1,
            name: "Demo".into(),
            ids: vec!["com.demo".into()],
        };
        let title = |loader: &Loader| loader.load(&app).groups[0].entries[0].title.clone();
        assert_eq!(title(&loader), "Open");
        loader.set_lang(Lang::Ja);
        assert_eq!(loader.lang(), Lang::Ja);
        assert_eq!(title(&loader), "開く");
        // Changing the user data must keep the language, and vice versa.
        loader.set_user_catalog(Arc::new(Catalog::default()));
        assert_eq!(loader.lang(), Lang::Ja);
        assert_eq!(title(&loader), "開く");
        loader.set_lang(Lang::En);
        assert_eq!(loader.lang(), Lang::En);
        assert_eq!(title(&loader), "Open");
    }
    #[test]
    fn reads_skip_running_and_fresh_reads() {
        let t0 = Instant::now();
        let mut reads = Reads::default();
        assert!(reads.start(1, t0));
        // The press started a read; showing the panel must not start another.
        assert!(!reads.start(1, t0 + Duration::from_millis(100)));
        // Other apps are independent.
        assert!(reads.start(2, t0));
        reads.finish(1, true, t0 + Duration::from_millis(900));
        assert!(!reads.start(1, t0 + Duration::from_millis(1000)));
        assert!(reads.start(1, t0 + Duration::from_millis(900) + Reads::FRESH));
    }

    #[test]
    fn a_lost_read_does_not_block_forever() {
        let t0 = Instant::now();
        let mut reads = Reads::default();
        assert!(reads.start(1, t0));
        assert!(!reads.start(1, t0 + Duration::from_secs(5)));
        assert!(reads.start(1, t0 + Reads::LOST));
    }

    #[test]
    fn failed_reads_are_not_fresh() {
        let t0 = Instant::now();
        let mut reads = Reads::default();
        assert!(reads.start(1, t0));
        reads.finish(1, false, t0);
        assert!(reads.start(1, t0 + Duration::from_millis(10)));
    }

    #[test]
    fn invalidate_forces_the_next_read() {
        let t0 = Instant::now();
        let mut reads = Reads::default();
        assert!(reads.start(1, t0));
        reads.finish(1, true, t0);
        assert!(reads.start(2, t0));
        reads.invalidate();
        assert!(reads.start(1, t0));
        assert!(reads.start(2, t0));
    }
}
