//! Starter file for user shortcut data, created from the panel's empty state.

use keylens_core::AppInfo;

/// File name and contents of a user data file for `app`, or `None` if it has no bundle ID.
/// The contents parse as valid data (no groups yet); an example group is left as comments.
pub fn user_data_template(app: &AppInfo) -> Option<(String, String)> {
    let id = app.ids.first()?;
    let file: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || ".-_".contains(*c))
        .collect();
    let name = toml_string(&app.name);
    let id = toml_string(id);
    let text = format!(
        "# Shortcuts for {name}. Format: see CONTRIBUTING.md (\"Contributing shortcut data\").\n\
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
}
