//! Validated community data.

use std::collections::{BTreeMap, HashMap};

use crate::community::raw::RawFile;
use crate::keys::Shortcut;
use crate::locale::{Lang, Localized};

#[derive(Clone, Debug, PartialEq)]
pub struct AppData {
    pub name: String,
    /// Bundle identifiers from `app.match.macos`.
    pub ids: Vec<String>,
    pub groups: Vec<DataGroup>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataGroup {
    pub title: Localized,
    pub shortcuts: Vec<DataShortcut>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataShortcut {
    pub title: Localized,
    keys: Shortcut,
}

impl DataShortcut {
    pub fn keys(&self) -> &Shortcut {
        &self.keys
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}: {message}")]
pub struct DataError {
    pub file: String,
    pub message: String,
}

pub fn parse_app_data(file: &str, text: &str) -> Result<AppData, DataError> {
    let fail = |message: String| DataError {
        file: file.to_string(),
        message,
    };
    let raw: RawFile = toml::from_str(text).map_err(|e| fail(e.to_string()))?;
    if raw.schema != 1 {
        return Err(fail(format!(
            "unsupported schema {} (expected 1)",
            raw.schema
        )));
    }
    if raw.app.name.trim().is_empty() {
        return Err(fail("app.name is empty".into()));
    }
    let ids = raw.app.matches.macos;
    if ids.is_empty() {
        return Err(fail("app.match.macos needs at least one bundle id".into()));
    }

    let mut seen: HashMap<Shortcut, String> = HashMap::new();
    let mut groups = Vec::new();
    for group in raw.group {
        let group_title = localized(&group.title, group.i18n)
            .map_err(|m| fail(format!("group `{}`: {m}", group.title)))?;
        if group.shortcut.is_empty() {
            return Err(fail(format!("group `{}` has no shortcuts", group.title)));
        }
        let mut shortcuts = Vec::new();
        for shortcut in group.shortcut {
            let context = format!("`{}` › `{}`", group.title, shortcut.title);
            let title = localized(&shortcut.title, shortcut.i18n)
                .map_err(|m| fail(format!("{context}: {m}")))?;
            let keys = Shortcut::parse_seq(&shortcut.keys.seq().parts())
                .map_err(|e| fail(format!("{context}: {e}")))?;
            if let Some(other) = seen.insert(keys.clone(), shortcut.title.clone()) {
                return Err(fail(format!(
                    "{context}: {} is already used by `{other}`",
                    keys.display()
                )));
            }
            shortcuts.push(DataShortcut { title, keys });
        }
        groups.push(DataGroup {
            title: group_title,
            shortcuts,
        });
    }
    Ok(AppData {
        name: raw.app.name,
        ids,
        groups,
    })
}

fn localized(en: &str, i18n: BTreeMap<String, String>) -> Result<Localized, String> {
    if en.trim().is_empty() {
        return Err("title is empty".into());
    }
    let mut translations = BTreeMap::new();
    for (key, text) in i18n {
        let lang = Lang::from_data_key(&key)
            .ok_or_else(|| format!("unknown language `{key}` (use zh-Hans, zh-Hant or ja)"))?;
        if text.trim().is_empty() {
            return Err(format!("`{key}` translation is empty"));
        }
        translations.insert(lang, text);
    }
    Ok(Localized {
        en: en.to_string(),
        translations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::Lang;

    const VALID: &str = r#"
schema = 1

[app]
name = "Visual Studio Code"

[app.match]
macos = ["com.microsoft.VSCode"]

[[group]]
title = "General"
i18n = { zh-Hans = "常规" }

  [[group.shortcut]]
  title = "Show Command Palette"
  i18n = { zh-Hans = "显示命令面板", ja = "コマンドパレットを表示" }
  keys = "Mod+Shift+P"

  [[group.shortcut]]
  title = "Open Keyboard Shortcuts"
  keys = ["Mod+K", "Mod+S"]

  [[group.shortcut]]
  title = "Move Line Up"
  keys = { default = "Alt+Up", macos = "Option+Up" }
"#;

    fn error(text: &str) -> String {
        parse_app_data("test.toml", text).unwrap_err().to_string()
    }

    #[test]
    fn parses_a_valid_file() {
        let app = parse_app_data("vscode.toml", VALID).unwrap();
        assert_eq!(app.name, "Visual Studio Code");
        assert_eq!(app.ids, ["com.microsoft.VSCode"]);
        let group = &app.groups[0];
        assert_eq!(group.title.get(Lang::ZhHans), "常规");
        let palette = &group.shortcuts[0];
        assert_eq!(palette.title.get(Lang::Ja), "コマンドパレットを表示");
        assert_eq!(palette.keys().display(), "⇧⌘P");
        assert_eq!(group.shortcuts[1].keys().display(), "⌘K ⌘S");
        assert_eq!(group.shortcuts[2].keys().display(), "⌥↑");
    }

    #[test]
    fn keys_table_falls_back_to_default_without_macos() {
        let text = VALID.replace(
            "keys = { default = \"Alt+Up\", macos = \"Option+Up\" }",
            "keys = { default = \"Ctrl+Up\" }",
        );
        let app = parse_app_data("test.toml", &text).unwrap();
        assert_eq!(app.groups[0].shortcuts[2].keys().display(), "⌃↑");
    }

    #[test]
    fn legacy_windows_and_linux_keys_are_ignored() {
        let text = VALID
            .replace(
                "macos = [\"com.microsoft.VSCode\"]",
                "macos = [\"com.microsoft.VSCode\"]\nwindows = [\"Code.exe\"]\nlinux = [\"code\"]",
            )
            .replace(
                "keys = { default = \"Alt+Up\", macos = \"Option+Up\" }",
                "keys = { default = \"Alt+Up\", macos = \"Option+Up\", windows = \"Ctrl+Up\", linux = \"Not+A+Key\" }",
            );
        let app = parse_app_data("legacy.toml", &text).unwrap();
        assert_eq!(app, parse_app_data("legacy.toml", VALID).unwrap());
        // A legacy key that duplicates another shortcut no longer counts as a conflict.
        let dup = VALID.replace(
            "keys = { default = \"Alt+Up\", macos = \"Option+Up\" }",
            "keys = { default = \"Alt+Up\", windows = \"Mod+Shift+P\", linux = \"Mod+Shift+P\" }",
        );
        assert!(parse_app_data("test.toml", &dup).is_ok());
    }

    #[test]
    fn errors_name_the_file() {
        assert!(error("schema = ").starts_with("test.toml: "));
    }

    #[test]
    fn rejects_unknown_fields_and_bad_keys() {
        assert!(
            error(&VALID.replace("title = \"General\"", "tilte = \"General\"")).contains("tilte")
        );
        let bad_key = error(&VALID.replace("Mod+Shift+P", "Mod+Shft+P"));
        assert!(
            bad_key.contains("Show Command Palette") && bad_key.contains("Shft"),
            "{bad_key}"
        );
    }

    #[test]
    fn keys_table_errors_explain_the_problem() {
        let line = "keys = { default = \"Alt+Up\", macos = \"Option+Up\" }";
        let typo =
            error(&VALID.replace(line, "keys = { default = \"Alt+Up\", mac = \"Option+Up\" }"));
        assert!(typo.contains("mac"), "{typo}");
        let missing = error(&VALID.replace(line, "keys = { macos = \"Option+Up\" }"));
        assert!(missing.contains("default"), "{missing}");
        let number = error(&VALID.replace(line, "keys = 5"));
        assert!(number.contains("string, array or table"), "{number}");
    }

    #[test]
    fn rejects_wrong_schema_and_missing_ids() {
        assert!(error(&VALID.replace("schema = 1", "schema = 2")).contains("schema"));
        let no_ids = VALID.replace("macos = [\"com.microsoft.VSCode\"]", "");
        assert!(error(&no_ids).contains("app.match"));
        let windows_only = VALID.replace(
            "macos = [\"com.microsoft.VSCode\"]",
            "windows = [\"Code.exe\"]",
        );
        assert!(error(&windows_only).contains("app.match"));
    }

    #[test]
    fn rejects_unknown_language_and_empty_titles() {
        assert!(error(&VALID.replace("zh-Hans = \"常规\"", "zh-CN = \"常规\"")).contains("zh-CN"));
        assert!(
            error(&VALID.replace("title = \"Move Line Up\"", "title = \"  \"")).contains("empty")
        );
    }

    #[test]
    fn rejects_duplicate_shortcuts() {
        let dup = VALID.replace("[\"Mod+K\", \"Mod+S\"]", "\"Mod+Shift+P\"");
        assert!(error(&dup).contains("already used"), "{}", error(&dup));
    }

    #[test]
    fn rejects_empty_groups() {
        let text = format!("{VALID}\n[[group]]\ntitle = \"Empty\"\n");
        assert!(error(&text).contains("no shortcuts"));
    }
}
