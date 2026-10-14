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

impl ShortcutSource for CommunitySource {
    fn load(&self, app: &AppInfo) -> Result<Vec<Entry>, SourceError> {
        let Some(data) = self.catalog.find(self.os, &app.ids) else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        for group in &data.groups {
            for shortcut in &group.shortcuts {
                let title = shortcut.title.get(self.lang);
                entries.push(Entry {
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
                });
            }
        }
        Ok(entries)
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
    fn unknown_app_yields_nothing() {
        assert!(
            source(Os::MacOS, Lang::En)
                .load(&app("com.other"))
                .unwrap()
                .is_empty()
        );
    }
}
