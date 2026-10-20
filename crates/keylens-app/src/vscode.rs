//! VS Code family editors (VS Code, Insiders, Cursor): shows the user's own `keybindings.json`
//! customizations on top of the community data, instead of the plain community shortcuts.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use keylens_core::community::CommunitySource;
use keylens_core::vscode::keybindings::{Binding, parse_keybindings};
use keylens_core::vscode::overlay::apply;
use keylens_core::{AppInfo, Entry, Lang, Os, ShortcutSource, SourceError};

/// A VS Code family editor; each keeps its own user settings folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flavor {
    Code,
    Insiders,
    Cursor,
}

impl Flavor {
    pub const ALL: [Flavor; 3] = [Flavor::Code, Flavor::Insiders, Flavor::Cursor];

    /// Name of the editor's folder under the per-user application data folder.
    pub fn dir_name(self) -> &'static str {
        match self {
            Flavor::Code => "Code",
            Flavor::Insiders => "Code - Insiders",
            Flavor::Cursor => "Cursor",
        }
    }

    /// App ids (bundle id, executable name) of this editor on `os`.
    fn ids(self, os: Os) -> &'static [&'static str] {
        match (self, os) {
            (Flavor::Code, Os::MacOS) => &["com.microsoft.VSCode"],
            (Flavor::Insiders, Os::MacOS) => &["com.microsoft.VSCodeInsiders"],
            // Cursor's macOS bundle id has not been verified yet, so it is not matched there.
            (Flavor::Cursor, Os::MacOS) => &[],
            (Flavor::Code, Os::Windows) => &["Code.exe"],
            (Flavor::Insiders, Os::Windows) => &["Code - Insiders.exe"],
            (Flavor::Cursor, Os::Windows) => &["Cursor.exe"],
            (Flavor::Code, Os::Linux) => &["code"],
            (Flavor::Insiders, Os::Linux) => &["code-insiders"],
            (Flavor::Cursor, Os::Linux) => &["cursor"],
        }
    }

    /// The editor `app` is, if it is one of the VS Code family.
    pub fn for_app(app: &AppInfo, os: Os) -> Option<Flavor> {
        Flavor::ALL.into_iter().find(|flavor| {
            app.ids
                .iter()
                .any(|id| flavor.ids(os).contains(&id.as_str()))
        })
    }
}

/// The user `keybindings.json` of `flavor`: under `~/Library/Application Support` on macOS,
/// `%APPDATA%` on Windows (falling back to `~/AppData/Roaming`) and `~/.config` on Linux.
pub fn keybindings_path(flavor: Flavor, os: Os, home: &Path, appdata: Option<&Path>) -> PathBuf {
    let base = match os {
        Os::MacOS => home.join("Library/Application Support"),
        Os::Windows => appdata
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home.join("AppData").join("Roaming")),
        Os::Linux => home.join(".config"),
    };
    base.join(flavor.dir_name())
        .join("User")
        .join("keybindings.json")
}

/// Where the editors' settings folders are found: [`Locations::from_env`] in the app; tests point
/// `home` at a temporary folder instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locations {
    pub home: PathBuf,
    /// `%APPDATA%` (only used on Windows).
    pub appdata: Option<PathBuf>,
}

impl Locations {
    pub fn from_env() -> Self {
        Self {
            home: std::env::home_dir().unwrap_or_else(crate::config::home_dir),
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
        }
    }

    pub fn keybindings_path(&self, flavor: Flavor, os: Os) -> PathBuf {
        keybindings_path(flavor, os, &self.home, self.appdata.as_deref())
    }

    /// The `keybindings.json` files of all flavors, for change detection.
    pub fn watched_paths(&self, os: Os) -> Vec<PathBuf> {
        Flavor::ALL
            .into_iter()
            .map(|flavor| self.keybindings_path(flavor, os))
            .collect()
    }
}

/// The `keybindings.json` files of all flavors in the user's environment.
pub fn watched_paths(os: Os) -> Vec<PathBuf> {
    Locations::from_env().watched_paths(os)
}

/// Community shortcuts (user data over built-in data) with the user's `keybindings.json` applied.
/// A missing or unreadable file means no customizations.
pub struct KeybindingsSource {
    user: Arc<CommunitySource>,
    builtin: Arc<CommunitySource>,
    os: Os,
    /// Title of the group for commands that have no community entry, in the data language.
    custom_group: String,
    locations: Locations,
}

impl KeybindingsSource {
    pub fn new(
        user: Arc<CommunitySource>,
        builtin: Arc<CommunitySource>,
        os: Os,
        lang: Lang,
        locations: Locations,
    ) -> Self {
        Self {
            user,
            builtin,
            os,
            custom_group: rust_i18n::t!("Group.MyCustom", locale = ui_locale(lang)).to_string(),
            locations,
        }
    }

    fn bindings(&self, app: &AppInfo) -> Vec<Binding> {
        let Some(flavor) = Flavor::for_app(app, self.os) else {
            return Vec::new();
        };
        let path = self.locations.keybindings_path(flavor, self.os);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(error) => {
                tracing::warn!(path = %path.display(), "cannot read keybindings.json: {error}");
                return Vec::new();
            }
        };
        match parse_keybindings(&text, self.os) {
            Ok(parsed) => {
                if parsed.skipped > 0 {
                    tracing::debug!(
                        path = %path.display(),
                        skipped = parsed.skipped,
                        "keybindings.json entries with unknown keys were skipped"
                    );
                }
                parsed.bindings
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), "{error}");
                Vec::new()
            }
        }
    }
}

/// The UI locale whose strings match the data language `lang`.
fn ui_locale(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "en",
        Lang::ZhHans => "zh-CN",
        Lang::ZhHant => "zh-TW",
        Lang::Ja => "ja",
    }
}

impl ShortcutSource for KeybindingsSource {
    fn load(&self, app: &AppInfo) -> Result<Vec<Entry>, SourceError> {
        let layers = vec![
            self.user.load_with_commands(app),
            self.builtin.load_with_commands(app),
        ];
        Ok(apply(layers, &self.bindings(app), &self.custom_group))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::EntrySource;
    use keylens_core::community::Catalog;

    const VSCODE_DATA: &str = r#"
schema = 1
[app]
name = "Visual Studio Code"
[app.match]
macos = ["com.microsoft.VSCode"]
[[group]]
title = "General"
[[group.shortcut]]
title = "Show Command Palette"
keys = "Mod+Shift+P"
command = "workbench.action.showCommands"
[[group.shortcut]]
title = "Save"
keys = "Mod+S"
command = "workbench.action.files.save"
"#;

    fn app(ids: &[&str]) -> AppInfo {
        AppInfo {
            pid: 1,
            name: "Editor".into(),
            ids: ids.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keylens-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn source(home: &Path, lang: Lang) -> KeybindingsSource {
        let (builtin, errors) = Catalog::from_files([("vscode.toml", VSCODE_DATA)]);
        assert!(errors.is_empty(), "{errors:?}");
        let os = Os::MacOS;
        let community = |catalog: Catalog, source| {
            Arc::new(CommunitySource::new(Arc::new(catalog), os, lang, source))
        };
        KeybindingsSource::new(
            community(Catalog::default(), EntrySource::User),
            community(builtin, EntrySource::Community),
            os,
            lang,
            Locations {
                home: home.to_path_buf(),
                appdata: None,
            },
        )
    }

    fn keys_of(entries: &[Entry], title: &str) -> Vec<String> {
        entries
            .iter()
            .filter(|e| e.title == title)
            .map(|e| e.shortcut.display(Os::MacOS))
            .collect()
    }

    #[test]
    fn flavor_by_app_ids() {
        assert_eq!(
            Flavor::for_app(&app(&["Code.exe"]), Os::Windows),
            Some(Flavor::Code)
        );
        assert_eq!(
            Flavor::for_app(&app(&["com.microsoft.VSCodeInsiders"]), Os::MacOS),
            Some(Flavor::Insiders)
        );
        assert_eq!(
            Flavor::for_app(&app(&["Cursor.exe"]), Os::Windows),
            Some(Flavor::Cursor)
        );
        assert_eq!(
            Flavor::for_app(&app(&["code-insiders"]), Os::Linux),
            Some(Flavor::Insiders)
        );
        assert_eq!(Flavor::for_app(&app(&["chrome.exe"]), Os::Windows), None);
        // Ids only count on their own platform.
        assert_eq!(Flavor::for_app(&app(&["Code.exe"]), Os::MacOS), None);
    }

    #[test]
    fn keybindings_paths_per_os() {
        let home = Path::new("/h");
        assert_eq!(
            keybindings_path(Flavor::Code, Os::MacOS, home, None),
            Path::new("/h/Library/Application Support/Code/User/keybindings.json")
        );
        assert_eq!(
            keybindings_path(
                Flavor::Insiders,
                Os::Windows,
                home,
                Some(Path::new("C:/AppData/Roaming"))
            ),
            Path::new("C:/AppData/Roaming/Code - Insiders/User/keybindings.json")
        );
        assert_eq!(
            keybindings_path(Flavor::Cursor, Os::Linux, home, None),
            Path::new("/h/.config/Cursor/User/keybindings.json")
        );
    }

    #[test]
    fn watched_paths_cover_every_flavor() {
        assert_eq!(watched_paths(Os::MacOS).len(), Flavor::ALL.len());
    }

    #[test]
    fn source_applies_user_file_and_survives_bad_json() {
        let home = temp_home("vscode-source");
        let vscode = app(&["com.microsoft.VSCode"]);
        let src = source(&home, Lang::En);

        // No file yet: the community defaults.
        let entries = src.load(&vscode).unwrap();
        assert_eq!(keys_of(&entries, "Show Command Palette"), ["⇧⌘P"]);

        let path = keybindings_path(Flavor::Code, Os::MacOS, &home, None);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"// my keys
            [
              // What VS Code's keybinding editor writes when a key is changed.
              { "key": "ctrl+alt+p", "command": "workbench.action.showCommands" },
              { "key": "shift+cmd+p", "command": "-workbench.action.showCommands" },
              { "key": "cmd+j", "command": "gitlens.showCommitSearch" },
            ]"#,
        )
        .unwrap();
        let entries = src.load(&vscode).unwrap();
        assert_eq!(keys_of(&entries, "Show Command Palette"), ["⌃⌥P"]);
        let custom = entries.iter().find(|e| e.title == "Show Commit Search");
        assert_eq!(custom.map(|e| e.group.as_str()), Some("My Custom"));

        std::fs::write(&path, "[ { not json").unwrap();
        let entries = src.load(&vscode).unwrap();
        assert_eq!(keys_of(&entries, "Show Command Palette"), ["⇧⌘P"]);
        assert_eq!(
            keys_of(&entries, "Show Commit Search"),
            Vec::<String>::new()
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn custom_group_follows_the_language() {
        let home = temp_home("vscode-lang");
        let path = keybindings_path(Flavor::Code, Os::MacOS, &home, None);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"[{ "key": "cmd+j", "command": "foo.bar" }]"#).unwrap();
        let vscode = app(&["com.microsoft.VSCode"]);
        let group = |lang| {
            let entries = source(&home, lang).load(&vscode).unwrap();
            entries
                .iter()
                .find(|e| e.title == "Bar")
                .map(|e| e.group.clone())
        };
        assert_eq!(group(Lang::En).as_deref(), Some("My Custom"));
        assert_eq!(group(Lang::ZhHans).as_deref(), Some("我的自定义"));
        assert_eq!(group(Lang::ZhHant).as_deref(), Some("我的自訂"));
        assert_eq!(group(Lang::Ja).as_deref(), Some("マイカスタム"));
        std::fs::remove_dir_all(&home).unwrap();
    }
}
