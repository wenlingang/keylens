//! 用户配置（`~/.config/keylens/config.toml`）、界面语言选择和数据目录。

use std::path::{Path, PathBuf};

use keylens_core::{Locale, resolve, resolve_preferred};
use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 覆盖系统语言，例如 `"ja"`、`"zh-TW"`。
    pub language: Option<String>,
}

/// 未知的键会被忽略，这样旧版本能读新版本写的配置。
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

/// 配置里写了语言就用它（不认识的语言回退到英文），否则按系统首选语言选择。
pub fn choose_locale(config: &Config, system: &[String]) -> Locale {
    match &config.language {
        Some(tag) => resolve(tag),
        None => resolve_preferred(system.iter().map(String::as_str)),
    }
}

pub fn config_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("keylens")
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
    fn data_dir_is_under_config_dir() {
        assert!(user_data_dir().starts_with(config_dir()));
        assert!(config_path().ends_with("keylens/config.toml"));
    }
}
