//! User config (`~/.config/keylens/config.toml`), UI language selection and the data directory.

use std::path::{Path, PathBuf};

use keylens_core::{Locale, resolve, resolve_preferred};
use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Overrides the system language, e.g. `"ja"`, `"zh-TW"`.
    pub language: Option<String>,
}

/// Unknown keys are ignored so older versions can read configs written by newer ones.
pub fn parse_config(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

pub fn load_config(path: &Path) -> (Config, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse_config(&text) {
            Ok(config) => (config, None),
            Err(e) => (Config::default(), Some(format!("{}: {e}", path.display()))),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
        Err(e) => (Config::default(), Some(format!("{}: {e}", path.display()))),
    }
}

/// Use the language set in the config (unknown languages fall back to English); otherwise pick from the system's preferred languages.
pub fn choose_locale(config: &Config, system: &[String]) -> Locale {
    match &config.language {
        Some(tag) => resolve(tag),
        None => resolve_preferred(system.iter().map(String::as_str)),
    }
}

pub fn config_dir() -> PathBuf {
    config_dir_in(
        &home_dir(),
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
    )
}

/// `~/.config/keylens` on macOS. Elsewhere `$XDG_CONFIG_HOME/keylens` when that is set to an
/// absolute path (the XDG spec says to ignore relative ones), otherwise `~/.config/keylens`.
fn config_dir_in(home: &Path, xdg_config_home: Option<PathBuf>) -> PathBuf {
    let base = xdg_config_home
        .filter(|dir| cfg!(not(target_os = "macos")) && dir.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    base.join("keylens")
}

pub(crate) fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn user_data_dir() -> PathBuf {
    config_dir().join("apps")
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::Lang;

    #[test]
    fn parses_language_and_ignores_unknown_keys() {
        let config = parse_config("language = \"ja\"\nfuture_option = 1\n").unwrap();
        assert_eq!(config.language.as_deref(), Some("ja"));
        assert_eq!(parse_config("").unwrap(), Config::default());
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(parse_config("language = ").is_err());
        assert!(parse_config("language = 3").is_err());
    }

    #[test]
    fn missing_file_is_the_default_without_error() {
        let (config, error) = load_config(Path::new("/nonexistent/keylens/config.toml"));
        assert_eq!(config, Config::default());
        assert!(error.is_none());
    }

    #[test]
    fn broken_file_falls_back_and_reports() {
        let dir = std::env::temp_dir().join(format!("keylens-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "language = ").unwrap();
        let (config, error) = load_config(&path);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(config, Config::default());
        assert!(error.unwrap().contains("config.toml"));
    }

    #[test]
    fn configured_language_wins_over_system() {
        let config = Config {
            language: Some("zh-TW".into()),
        };
        let locale = choose_locale(&config, &["ja-JP".into()]);
        assert_eq!((locale.lang, locale.ui), (Lang::ZhHant, "zh-TW"));
    }

    #[test]
    fn system_languages_are_used_without_config() {
        let locale = choose_locale(&Config::default(), &["fr-FR".into(), "zh-Hans-CN".into()]);
        assert_eq!((locale.lang, locale.ui), (Lang::ZhHans, "zh-CN"));
        let locale = choose_locale(&Config::default(), &[]);
        assert_eq!(locale.lang, Lang::En);
    }

    #[test]
    fn unknown_configured_language_falls_back_to_english() {
        let config = Config {
            language: Some("klingon".into()),
        };
        assert_eq!(choose_locale(&config, &["ja-JP".into()]).lang, Lang::En);
    }

    #[test]
    fn config_dir_defaults_to_dot_config() {
        assert_eq!(
            config_dir_in(Path::new("/home/u"), None),
            Path::new("/home/u/.config/keylens")
        );
        // Relative XDG paths are invalid and ignored.
        assert_eq!(
            config_dir_in(Path::new("/home/u"), Some("rel".into())),
            Path::new("/home/u/.config/keylens")
        );
    }

    #[test]
    fn xdg_config_home_is_honored_outside_macos() {
        let dir = config_dir_in(Path::new("/home/u"), Some("/xdg".into()));
        if cfg!(target_os = "macos") {
            assert_eq!(dir, Path::new("/home/u/.config/keylens"));
        } else {
            assert_eq!(dir, Path::new("/xdg/keylens"));
        }
    }

    #[test]
    fn data_dir_is_under_config_dir() {
        assert!(user_data_dir().starts_with(config_dir()));
        assert!(config_path().ends_with("keylens/config.toml"));
    }
}
