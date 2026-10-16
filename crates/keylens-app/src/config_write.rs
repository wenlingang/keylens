//! Edits `config.toml` text in place with `toml_edit`, keeping comments, order and unknown keys.

use std::path::{Path, PathBuf};

use keylens_core::{Modifiers, Os};
use toml_edit::{DocumentMut, Item, TableLike, value};

use crate::config::TriggerMode;

/// One edit from the settings window. `None` restores the default by removing the key.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Mode(Option<TriggerMode>),
    Key(Option<Modifiers>),
    HoldMs(Option<u64>),
    DoubleTapMs(Option<u64>),
    HideOnRelease(Option<bool>),
    Language(Option<String>),
    AutoCheck(Option<bool>),
    /// The version the update dialog's "Skip This Version" chose.
    SkipVersion(Option<String>),
    /// Removes `language` and the whole `[trigger]` and `[update]` tables.
    ResetAll,
}

const TRIGGER: &str = "trigger";
const UPDATE: &str = "update";

pub fn apply(text: &str, change: &Change, os: Os) -> Result<String, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    match change {
        Change::Mode(mode) => set_in(&mut doc, TRIGGER, "mode", mode.map(|m| value(m.name()))),
        Change::Key(key) => set_in(
            &mut doc,
            TRIGGER,
            "key",
            key.map(|k| value(k.trigger_name(os))),
        ),
        Change::HoldMs(ms) => set_in(&mut doc, TRIGGER, "hold-ms", ms.map(|ms| value(ms as i64))),
        Change::DoubleTapMs(ms) => set_in(
            &mut doc,
            TRIGGER,
            "double-tap-ms",
            ms.map(|ms| value(ms as i64)),
        ),
        Change::HideOnRelease(hide) => {
            set_in(&mut doc, TRIGGER, "hide-on-release", hide.map(value))
        }
        Change::AutoCheck(on) => set_in(&mut doc, UPDATE, "auto-check", on.map(value)),
        Change::SkipVersion(version) => set_in(
            &mut doc,
            UPDATE,
            "skip-version",
            version.as_deref().map(value),
        ),
        Change::Language(Some(tag)) => put(doc.as_table_mut(), "language", value(tag.as_str())),
        Change::Language(None) => {
            doc.remove("language");
        }
        Change::ResetAll => {
            doc.remove("language");
            doc.remove(TRIGGER);
            doc.remove(UPDATE);
        }
    }
    Ok(doc.to_string())
}

/// Sets `key` to `item`, keeping the comments around an existing value: a plain `insert` would
/// drop the `# comment` above the key and the one after the value.
fn put(table: &mut dyn TableLike, key: &str, mut item: Item) {
    if let Some(slot) = table.get_mut(key) {
        if let (Some(old), Some(new)) = (slot.as_value(), item.as_value_mut()) {
            *new.decor_mut() = old.decor().clone();
        }
        *slot = item;
    } else {
        table.insert(key, item);
    }
}

/// Sets or removes `key` in the table `[name]` (a normal or inline table), dropping the table once
/// empty.
fn set_in(doc: &mut DocumentMut, name: &str, key: &str, new: Option<Item>) {
    match new {
        Some(item) => {
            let table = doc.entry(name).or_insert(toml_edit::table());
            // A non-table `trigger` or `update` fails to parse as a Config, which makes Prefs read-only, so
            // edits never reach here with one.
            if let Some(table) = table.as_table_like_mut() {
                put(table, key, item);
            }
        }
        None => {
            let empty = match doc.get_mut(name).and_then(Item::as_table_like_mut) {
                Some(table) => {
                    table.remove(key);
                    table.is_empty()
                }
                None => false,
            };
            if empty {
                doc.remove(name);
            }
        }
    }
}

/// Writes `text` to `path` through a temporary file and a rename, so a crash never leaves a
/// half-written config. A symlinked config (e.g. kept in a dotfiles repo) stays a symlink: the
/// file it points to is replaced instead.
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let target = resolve_target(path)?;
    let dir = target.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".into());
    let tmp = dir.join(format!(".{name}.keylens-tmp"));
    std::fs::write(&tmp, text)?;
    // The temp file starts with the umask's mode; keep the mode of the file it replaces.
    if let Ok(meta) = std::fs::metadata(&target) {
        std::fs::set_permissions(&tmp, meta.permissions())?;
    }
    std::fs::rename(&tmp, &target)
}

/// The file to write for `path`: a symlink is followed to its destination, even when that
/// destination does not exist yet, so the link itself is never replaced.
fn resolve_target(path: &Path) -> std::io::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(real) => Ok(real),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match std::fs::read_link(path) {
            // A dangling link: relative targets are relative to the link's own directory.
            Ok(link) => {
                let dir = path.parent().unwrap_or(Path::new(""));
                resolve_target(&dir.join(link))
            }
            // Not a link, just a missing file.
            Err(_) => Ok(path.to_path_buf()),
        },
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;

    const HAND_WRITTEN: &str = "\
# my keylens config
language = \"ja\" # I am learning Japanese

future-option = 1

[trigger]
# slower, I keep triggering it by accident
hold-ms = 1200
";

    #[test]
    fn edits_keep_comments_order_and_unknown_keys() {
        let text = apply(HAND_WRITTEN, &Change::Key(Some(Modifiers::ALT)), Os::MacOS).unwrap();
        assert!(text.contains("# my keylens config"));
        assert!(text.contains("# I am learning Japanese"));
        assert!(text.contains("# slower, I keep triggering it by accident"));
        assert!(text.contains("future-option = 1"));
        assert!(text.contains("key = \"option\""));
        assert!(text.find("hold-ms").unwrap() < text.find("key = ").unwrap());
    }

    #[test]
    fn update_settings_go_to_their_own_table() {
        let text = apply("", &Change::AutoCheck(Some(false)), Os::MacOS).unwrap();
        let text = apply(&text, &Change::SkipVersion(Some("0.2.0".into())), Os::MacOS).unwrap();
        let (update, warnings) = parse_config(&text).unwrap().update_settings();
        assert!(warnings.is_empty());
        assert!(!update.auto_check);
        assert_eq!(update.skip_version.as_deref(), Some("0.2.0"));
        assert!(text.contains("[update]"), "{text}");

        let text = apply(&text, &Change::AutoCheck(None), Os::MacOS).unwrap();
        let text = apply(&text, &Change::SkipVersion(None), Os::MacOS).unwrap();
        assert!(
            !text.contains("update"),
            "an empty [update] table is dropped: {text}"
        );
    }

    #[test]
    fn reset_all_clears_update_settings_too() {
        let text = "[update]\nauto-check = false\n";
        let text = apply(text, &Change::ResetAll, Os::MacOS).unwrap();
        assert!(text.trim().is_empty(), "{text}");
    }

    #[test]
    fn key_is_written_in_the_platform_words() {
        let mac = apply("", &Change::Key(Some(Modifiers::META)), Os::MacOS).unwrap();
        let linux = apply("", &Change::Key(Some(Modifiers::META)), Os::Linux).unwrap();
        assert!(mac.contains("key = \"cmd\""), "{mac}");
        assert!(linux.contains("key = \"super\""), "{linux}");
    }

    #[test]
    fn every_change_round_trips_through_the_parser() {
        let changes = [
            Change::Mode(Some(TriggerMode::DoubleTap)),
            Change::Key(Some(Modifiers::SHIFT)),
            Change::HoldMs(Some(1500)),
            Change::DoubleTapMs(Some(275)),
            Change::HideOnRelease(Some(true)),
            Change::Language(Some("zh-TW".into())),
        ];
        let text = changes
            .iter()
            .fold(String::new(), |text, c| apply(&text, c, Os::MacOS).unwrap());
        let config = parse_config(&text).unwrap();
        assert_eq!(config.language.as_deref(), Some("zh-TW"));
        let t = &config.trigger;
        assert_eq!(t.mode.as_ref().unwrap().as_str(), Some("double-tap"));
        assert_eq!(t.key.as_ref().unwrap().as_str(), Some("shift"));
        assert_eq!(t.hold_ms.as_ref().unwrap().as_integer(), Some(1500));
        assert_eq!(t.double_tap_ms.as_ref().unwrap().as_integer(), Some(275));
        assert_eq!(t.hide_on_release.as_ref().unwrap().as_bool(), Some(true));
    }

    #[test]
    fn restoring_defaults_removes_keys_and_the_empty_table() {
        let text = apply(HAND_WRITTEN, &Change::HoldMs(None), Os::MacOS).unwrap();
        assert!(!text.contains("hold-ms"), "{text}");
        assert!(!text.contains("[trigger]"), "{text}");
        assert!(text.contains("future-option = 1"));

        let text = apply(HAND_WRITTEN, &Change::Language(None), Os::MacOS).unwrap();
        assert!(!text.contains("language"), "{text}");
    }

    #[test]
    fn reset_all_keeps_unknown_keys() {
        let text = apply(HAND_WRITTEN, &Change::ResetAll, Os::MacOS).unwrap();
        assert!(!text.contains("language"));
        assert!(!text.contains("[trigger]"));
        assert!(text.contains("future-option = 1"));
    }

    #[test]
    fn replacing_a_value_keeps_its_comments() {
        let text = "\
# above language
language = \"ja\" # trailing language
[trigger]
# above hold
hold-ms = 1200 # trailing hold
";
        let text = apply(text, &Change::Language(Some("zh-TW".into())), Os::MacOS).unwrap();
        let text = apply(&text, &Change::HoldMs(Some(900)), Os::MacOS).unwrap();
        assert!(text.contains("# above language"), "{text}");
        assert!(text.contains("# trailing language"), "{text}");
        assert!(text.contains("# above hold"), "{text}");
        assert!(text.contains("# trailing hold"), "{text}");
        assert_eq!(
            parse_config(&text).unwrap().language.as_deref(),
            Some("zh-TW")
        );
    }

    #[test]
    fn inline_trigger_tables_are_edited_too() {
        let text = apply(
            "trigger = { key = \"cmd\" }\n",
            &Change::HoldMs(Some(900)),
            Os::MacOS,
        )
        .unwrap();
        let config = parse_config(&text).unwrap();
        assert_eq!(config.trigger.hold_ms.unwrap().as_integer(), Some(900));
        assert_eq!(config.trigger.key.unwrap().as_str(), Some("cmd"));
    }

    #[test]
    fn invalid_text_is_an_error() {
        assert!(apply("language = ", &Change::ResetAll, Os::MacOS).is_err());
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("keylens-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn write_atomic_creates_missing_directories() {
        let dir = temp_dir("write-new");
        let path = dir.join("nested").join("config.toml");
        write_atomic(&path, "language = \"en\"\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "language = \"en\"\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_follows_symlinks() {
        let dir = temp_dir("write-link");
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        let real = dir.join("dotfiles").join("keylens.toml");
        std::fs::write(&real, "old").unwrap();
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        write_atomic(&link, "new").unwrap();

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_follows_dangling_symlinks() {
        let dir = temp_dir("write-dangling");
        std::fs::create_dir_all(dir.join("dotfiles")).unwrap();
        let link = dir.join("config.toml");
        // Relative target that does not exist yet: resolved against the link's own directory.
        std::os::unix::fs::symlink("dotfiles/keylens.toml", &link).unwrap();

        write_atomic(&link, "new").unwrap();

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("dotfiles").join("keylens.toml")).unwrap(),
            "new"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_keeps_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("write-mode");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        write_atomic(&path, "new").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
