//! Converts community data into shortcut entries.

use std::sync::Arc;

use crate::community::catalog::Catalog;
use crate::keys::Os;
use crate::locale::Lang;
use crate::model::{AppInfo, Entry, EntryId, EntrySource};
use crate::source::{ShortcutSource, SourceError};

pub struct CommunitySource {
    catalog: Arc<Catalog>,
    os: Os,
    lang: Lang,
    source: EntrySource,
}

impl CommunitySource {
    /// `source` distinguishes built-in data (`Community`) from user data (`User`).
    pub fn new(catalog: Arc<Catalog>, os: Os, lang: Lang, source: EntrySource) -> Self {
        Self {
            catalog,
            os,
            lang,
            source,
        }
    }
}

impl CommunitySource {
    /// Same entries as `load`, each paired with its VS Code command ID (if any).
    pub fn load_with_commands(&self, app: &AppInfo) -> Vec<(Entry, Option<String>)> {
        let Some(data) = self.catalog.find(self.os, &app.ids) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        for group in &data.groups {
            for shortcut in &group.shortcuts {
                let title = shortcut.title.get(self.lang);
                let entry = Entry {
                    id: EntryId {
                        source: self.source,
                        snapshot: 0,
                        index: entries.len() as u32,
                    },
                    group: group.title.get(self.lang).to_string(),
                    path: Vec::new(),
                    title: title.to_string(),
                    search_terms: shortcut
                        .title
                        .all()
                        .filter(|t| *t != title)
                        .map(String::from)
                        .collect(),
                    shortcut: shortcut.keys(self.os).clone(),
                    enabled: true,
                };
                entries.push((entry, shortcut.command.clone()));
            }
        }
        entries
    }
}

impl ShortcutSource for CommunitySource {
    fn load(&self, app: &AppInfo) -> Result<Vec<Entry>, SourceError> {
        Ok(self
            .load_with_commands(app)
            .into_iter()
            .map(|(entry, _)| entry)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA: &str = r#"
schema = 1
[app]
name = "Demo"
[app.match]
macos = ["com.demo"]
windows = ["demo.exe"]
[[group]]
title = "General"
i18n = { zh-Hans = "常规" }
[[group.shortcut]]
title = "Save"
i18n = { zh-Hans = "保存", ja = "保存する" }
keys = "Mod+S"
"#;

    fn source(os: Os, lang: Lang) -> CommunitySource {
        let (catalog, errors) = Catalog::from_files([("demo.toml", DATA)]);
        assert!(errors.is_empty());
        CommunitySource::new(Arc::new(catalog), os, lang, EntrySource::Community)
    }

    fn app(id: &str) -> AppInfo {
        AppInfo {
            pid: 7,
            name: "Demo".into(),
            ids: vec![id.into()],
        }
    }

    #[test]
    fn produces_localized_entries_with_other_languages_searchable() {
        let entries = source(Os::MacOS, Lang::ZhHans)
            .load(&app("com.demo"))
            .unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!((e.group.as_str(), e.title.as_str()), ("常规", "保存"));
        assert_eq!(e.search_terms, ["Save", "保存する"]);
        assert_eq!(e.shortcut.display(Os::MacOS), "⌘S");
        assert_eq!(
            e.id,
            EntryId {
                source: EntrySource::Community,
                snapshot: 0,
                index: 0
            }
        );
        assert!(e.enabled);
    }

    #[test]
    fn uses_platform_specific_keys_and_ids() {
        let entries = source(Os::Windows, Lang::En)
            .load(&app("demo.exe"))
            .unwrap();
        assert_eq!(entries[0].shortcut.display(Os::Windows), "Ctrl+S");
        assert!(
            source(Os::Linux, Lang::En)
                .load(&app("demo.exe"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn load_with_commands_pairs_entries_with_their_command() {
        let text = format!(
            "{DATA}command = \"workbench.action.files.save\"\n[[group.shortcut]]\ntitle = \"Open\"\nkeys = \"Mod+O\"\n"
        );
        let (catalog, errors) = Catalog::from_files([("demo.toml", text.as_str())]);
        assert!(errors.is_empty(), "{errors:?}");
        let src = CommunitySource::new(Arc::new(catalog), Os::Windows, Lang::En, EntrySource::User);
        let with = src.load_with_commands(&app("demo.exe"));
        let commands: Vec<_> = with.iter().map(|(_, c)| c.as_deref()).collect();
        assert_eq!(commands, [Some("workbench.action.files.save"), None]);
        let entries: Vec<_> = with.into_iter().map(|(e, _)| e).collect();
        assert_eq!(entries, src.load(&app("demo.exe")).unwrap());
        assert_eq!(entries[1].id.index, 1);
        assert!(src.load_with_commands(&app("other")).is_empty());
    }

    #[test]
    fn unknown_app_yields_nothing() {
        assert!(
            source(Os::MacOS, Lang::En)
                .load(&app("com.other"))
                .unwrap()
                .is_empty()
        );
    }
}
