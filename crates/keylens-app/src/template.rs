//! Starter file for user shortcut data, created from the panel's empty state.

use std::io::Write;
use std::path::{Path, PathBuf};

use keylens_core::AppInfo;

/// File name and contents of a user data file for `app`, or `None` if it has no usable bundle
/// ID. The contents parse as valid data (no groups yet); an example group is left as comments.
pub fn user_data_template(app: &AppInfo) -> Option<(String, String)> {
    let id = app.ids.first()?;
    let file: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || ".-_".contains(*c))
        .collect();
    if file.trim_matches('.').is_empty() {
        return None;
    }
    let name = toml_string(&app.name);
    let id = toml_string(id);
    let text = format!(
        "# Keylens shortcuts for {name}.\n\
         # Save this file, then restart Keylens (menu bar ⌘ › Quit Keylens, then open it again)\n\
         # to load your changes.\n\
         #\n\
         # Keys: modifiers Mod (⌘ on macOS), Cmd, Ctrl, Alt/Option, Shift, Fn joined with \"+\",\n\
         # e.g. \"Mod+Shift+P\"; a sequence is an array: [\"Mod+K\", \"Mod+S\"].\n\
         # Titles are in English; add translations with i18n = {{ zh-Hans = \"...\" }}.\n\
         schema = 1\n\n\
         [app]\n\
         name = {name}\n\n\
         [app.match]\n\
         macos = [{id}]\n\n\
         # [[group]]\n\
         # title = \"General\"\n\
         #\n\
         #   [[group.shortcut]]\n\
         #   title = \"Do something\"\n\
         #   keys = \"Mod+Shift+K\"\n"
    );
    Some((format!("{file}.toml"), text))
}

/// Creates the user data file for `app` in `dir` unless it already exists, and returns its
/// path (`None` if the app has no usable bundle ID). Existing files are never overwritten.
pub fn create_user_file(dir: &Path, app: &AppInfo) -> std::io::Result<Option<PathBuf>> {
    let Some((file, text)) = user_data_template(app) else {
        return Ok(None);
    };
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut f) => f.write_all(text.as_bytes())?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    Ok(Some(path))
}

/// A TOML basic string literal.
fn toml_string(value: &str) -> String {
    let escaped: String = value
        .chars()
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            c => vec![c],
        })
        .collect();
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::community::parse_app_data;

    fn app(ids: &[&str]) -> AppInfo {
        AppInfo {
            pid: 1,
            name: "Notion \"Beta\"".into(),
            ids: ids.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn template_parses_and_matches_the_app() {
        let (file, text) = user_data_template(&app(&["notion.id"])).unwrap();
        assert_eq!(file, "notion.id.toml");
        let data = parse_app_data(&file, &text).unwrap();
        assert_eq!(data.name, "Notion \"Beta\"");
        assert_eq!(data.ids.macos, ["notion.id"]);
        assert!(data.groups.is_empty());
    }

    #[test]
    fn file_name_is_sanitized_and_ids_are_required() {
        let (file, _) = user_data_template(&app(&["com.example/../x y"])).unwrap();
        assert_eq!(file, "com.example..xy.toml");
        assert!(user_data_template(&app(&[])).is_none());
    }

    #[test]
    fn template_tells_the_user_to_restart_and_explains_the_format() {
        let (_, text) = user_data_template(&app(&["notion.id"])).unwrap();
        assert!(text.contains("restart Keylens"), "{text}");
        assert!(text.contains("Mod"), "{text}");
        assert!(!text.contains("CONTRIBUTING"), "{text}");
    }

    #[test]
    fn an_id_without_usable_characters_has_no_file() {
        assert!(user_data_template(&app(&["///"])).is_none());
        assert!(user_data_template(&app(&["..."])).is_none());
    }

    #[test]
    fn creating_the_file_never_overwrites_existing_content() {
        let dir = std::env::temp_dir().join(format!("keylens-template-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = create_user_file(&dir, &app(&["notion.id"]))
            .unwrap()
            .unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("notion.id")
        );
        std::fs::write(&path, "# my edits\n").unwrap();
        let again = create_user_file(&dir, &app(&["notion.id"]))
            .unwrap()
            .unwrap();
        assert_eq!(again, path);
        let kept = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(kept, "# my edits\n");
        assert!(create_user_file(&dir, &app(&[])).unwrap().is_none());
    }
}
