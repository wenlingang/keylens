//! 校验后的社区数据。

use std::collections::{BTreeMap, HashMap};

use crate::community::raw::{RawFile, RawKeys, RawSeq};
use crate::keys::{Os, Shortcut};
use crate::locale::{Lang, Localized};

#[derive(Clone, Debug, PartialEq)]
pub struct AppData {
    pub name: String,
    pub ids: AppIds,
    pub groups: Vec<DataGroup>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppIds {
    pub macos: Vec<String>,
    pub windows: Vec<String>,
    pub linux: Vec<String>,
}

impl AppIds {
    pub fn for_os(&self, os: Os) -> &[String] {
        match os {
            Os::MacOS => &self.macos,
            Os::Windows => &self.windows,
            Os::Linux => &self.linux,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataGroup {
    pub title: Localized,
    pub shortcuts: Vec<DataShortcut>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataShortcut {
    pub title: Localized,
    /// 按 `os_index` 顺序存放三个平台各自的快捷键。
    keys: [Shortcut; 3],
}

impl DataShortcut {
    pub fn keys(&self, os: Os) -> &Shortcut {
        &self.keys[os_index(os)]
    }
}

fn os_index(os: Os) -> usize {
    match os {
        Os::MacOS => 0,
        Os::Windows => 1,
        Os::Linux => 2,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}: {message}")]
pub struct DataError {
    pub file: String,
    pub message: String,
}

pub fn parse_app_data(file: &str, text: &str) -> Result<AppData, DataError> {
    let fail = |message: String| DataError { file: file.to_string(), message };
    let raw: RawFile = toml::from_str(text).map_err(|e| fail(e.to_string()))?;
    if raw.schema != 1 {
        return Err(fail(format!("unsupported schema {} (expected 1)", raw.schema)));
    }
    if raw.app.name.trim().is_empty() {
        return Err(fail("app.name is empty".into()));
    }
    let ids = AppIds {
        macos: raw.app.matches.macos,
        windows: raw.app.matches.windows,
        linux: raw.app.matches.linux,
    };
    if Os::ALL.iter().all(|os| ids.for_os(*os).is_empty()) {
        return Err(fail("app.match needs at least one id".into()));
    }

    let mut seen: [HashMap<Shortcut, String>; 3] = Default::default();
    let mut groups = Vec::new();
    for group in raw.group {
        let group_title =
            localized(&group.title, group.i18n).map_err(|m| fail(format!("group `{}`: {m}", group.title)))?;
        if group.shortcut.is_empty() {
            return Err(fail(format!("group `{}` has no shortcuts", group.title)));
        }
        let mut shortcuts = Vec::new();
        for shortcut in group.shortcut {
            let context = format!("`{}` › `{}`", group.title, shortcut.title);
            let title = localized(&shortcut.title, shortcut.i18n).map_err(|m| fail(format!("{context}: {m}")))?;
            let keys = resolve_keys(&shortcut.keys).map_err(|e| fail(format!("{context}: {e}")))?;
            for os in Os::ALL {
                if ids.for_os(os).is_empty() {
                    continue;
                }
                let key = &keys[os_index(os)];
                if let Some(other) = seen[os_index(os)].insert(key.clone(), shortcut.title.clone()) {
                    return Err(fail(format!(
                        "{context}: {} on {os:?} is already used by `{other}`",
                        key.display(os)
                    )));
                }
            }
            shortcuts.push(DataShortcut { title, keys });
        }
        groups.push(DataGroup { title: group_title, shortcuts });
    }
    Ok(AppData { name: raw.app.name, ids, groups })
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
    Ok(Localized { en: en.to_string(), translations })
}

fn pick(keys: &RawKeys, os: Os) -> &RawSeq {
    match keys {
        RawKeys::Same(seq) => seq,
        RawKeys::PerOs(per_os) => match os {
            Os::MacOS => per_os.macos.as_ref(),
            Os::Windows => per_os.windows.as_ref(),
            Os::Linux => per_os.linux.as_ref(),
        }
        .unwrap_or(&per_os.default),
    }
}

fn resolve_keys(keys: &RawKeys) -> Result<[Shortcut; 3], crate::keys::KeyParseError> {
    Ok([
        Shortcut::parse_seq(&pick(keys, Os::MacOS).parts(), Os::MacOS)?,
        Shortcut::parse_seq(&pick(keys, Os::Windows).parts(), Os::Windows)?,
        Shortcut::parse_seq(&pick(keys, Os::Linux).parts(), Os::Linux)?,
    ])
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
windows = ["Code.exe"]

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
        assert_eq!(app.ids.for_os(Os::MacOS), ["com.microsoft.VSCode"]);
        assert!(app.ids.for_os(Os::Linux).is_empty());
        let group = &app.groups[0];
        assert_eq!(group.title.get(Lang::ZhHans), "常规");
        let palette = &group.shortcuts[0];
        assert_eq!(palette.title.get(Lang::Ja), "コマンドパレットを表示");
        assert_eq!(palette.keys(Os::MacOS).display(Os::MacOS), "⇧⌘P");
        assert_eq!(palette.keys(Os::Windows).display(Os::Windows), "Ctrl+Shift+P");
        assert_eq!(group.shortcuts[1].keys(Os::MacOS).display(Os::MacOS), "⌘K ⌘S");
        assert_eq!(group.shortcuts[2].keys(Os::MacOS).display(Os::MacOS), "⌥↑");
        assert_eq!(group.shortcuts[2].keys(Os::Linux).display(Os::Linux), "Alt+Up");
    }

    #[test]
    fn errors_name_the_file() {
        assert!(error("schema = ").starts_with("test.toml: "));
    }

    #[test]
    fn rejects_unknown_fields_and_bad_keys() {
        assert!(error(&VALID.replace("title = \"General\"", "tilte = \"General\"")).contains("tilte"));
        let bad_key = error(&VALID.replace("Mod+Shift+P", "Mod+Shft+P"));
        assert!(bad_key.contains("Show Command Palette") && bad_key.contains("Shft"), "{bad_key}");
    }

    #[test]
    fn rejects_wrong_schema_and_missing_ids() {
        assert!(error(&VALID.replace("schema = 1", "schema = 2")).contains("schema"));
        let no_ids = VALID
            .replace("macos = [\"com.microsoft.VSCode\"]", "")
            .replace("windows = [\"Code.exe\"]", "");
        assert!(error(&no_ids).contains("app.match"));
    }

    #[test]
    fn rejects_unknown_language_and_empty_titles() {
        assert!(error(&VALID.replace("zh-Hans = \"常规\"", "zh-CN = \"常规\"")).contains("zh-CN"));
        assert!(error(&VALID.replace("title = \"Move Line Up\"", "title = \"  \"")).contains("empty"));
    }

    #[test]
    fn rejects_duplicate_shortcuts_on_platforms_with_ids() {
        let dup = VALID.replace("[\"Mod+K\", \"Mod+S\"]", "\"Mod+Shift+P\"");
        assert!(error(&dup).contains("already used"), "{}", error(&dup));
        let linux_only = VALID.replace(
            "keys = { default = \"Alt+Up\", macos = \"Option+Up\" }",
            "keys = { default = \"Alt+Up\", windows = \"Ctrl+Up\", linux = \"Mod+Shift+P\" }",
        );
        assert!(parse_app_data("test.toml", &linux_only).is_ok());
    }

    #[test]
    fn rejects_empty_groups() {
        let text = format!("{VALID}\n[[group]]\ntitle = \"Empty\"\n");
        assert!(error(&text).contains("no shortcuts"));
    }
}
