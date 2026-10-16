//! User config (`config.toml` in [`config_dir`]): the trigger, the UI language, and the data
//! directory.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::Duration;

use keylens_core::{Locale, Modifiers, Trigger, resolve, resolve_preferred};
use serde::Deserialize;

pub const HOLD_MS: RangeInclusive<u64> = 300..=2000;
pub const HOLD_STEP: u64 = 50;
pub const DOUBLE_TAP_MS: RangeInclusive<u64> = 200..=600;
pub const DOUBLE_TAP_STEP: u64 = 25;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Overrides the system language, e.g. `"ja"`, `"zh-TW"`.
    pub language: Option<String>,
    pub trigger: TriggerConfig,
}

/// `[trigger]`. Values stay raw so that one bad field only resets itself (see
/// [`Config::trigger_settings`]) instead of making the whole file unreadable.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct TriggerConfig {
    pub mode: Option<toml::Value>,
    pub key: Option<toml::Value>,
    pub hold_ms: Option<toml::Value>,
    pub double_tap_ms: Option<toml::Value>,
    pub hide_on_release: Option<toml::Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerMode {
    Hold,
    DoubleTap,
}

impl TriggerMode {
    pub fn name(self) -> &'static str {
        match self {
            TriggerMode::Hold => "hold",
            TriggerMode::DoubleTap => "double-tap",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "hold" => Some(TriggerMode::Hold),
            "double-tap" => Some(TriggerMode::DoubleTap),
            _ => None,
        }
    }
}

/// Everything the settings window shows about the trigger. Keeps both durations, although a
/// [`Trigger`] only uses the one that matches its mode, so switching modes keeps the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerSettings {
    pub mode: TriggerMode,
    pub key: Modifiers,
    pub hold_ms: u64,
    pub double_tap_ms: u64,
    /// Hide the panel when the held trigger key is released (hold mode only).
    pub hide_on_release: bool,
}

impl TriggerSettings {
    /// The platform's default trigger, with the built-in duration for the mode it does not use.
    pub fn defaults(platform: &Trigger) -> Self {
        let hold = Trigger::DEFAULT_HOLD_DELAY.as_millis() as u64;
        let tap = Trigger::DEFAULT_DOUBLE_TAP_INTERVAL.as_millis() as u64;
        match platform {
            Trigger::HoldModifier { key, delay } => Self {
                mode: TriggerMode::Hold,
                key: *key,
                hold_ms: delay.as_millis() as u64,
                double_tap_ms: tap,
                hide_on_release: false,
            },
            Trigger::DoubleTap { key, interval } => Self {
                mode: TriggerMode::DoubleTap,
                key: *key,
                hold_ms: hold,
                double_tap_ms: interval.as_millis() as u64,
                hide_on_release: false,
            },
            // Not configurable from the settings window; fall back to the macOS default.
            Trigger::Hotkey(_) | Trigger::External => Self {
                mode: TriggerMode::Hold,
                key: Modifiers::META,
                hold_ms: hold,
                double_tap_ms: tap,
                hide_on_release: false,
            },
        }
    }

    pub fn trigger(&self) -> Trigger {
        match self.mode {
            TriggerMode::Hold => Trigger::HoldModifier {
                key: self.key,
                delay: Duration::from_millis(self.hold_ms),
            },
            TriggerMode::DoubleTap => Trigger::DoubleTap {
                key: self.key,
                interval: Duration::from_millis(self.double_tap_ms),
            },
        }
    }
}

impl Config {
    /// The trigger settings on top of the platform default, plus a warning for every field that
    /// was ignored or clamped. Fields are independent: a bad one falls back alone.
    pub fn trigger_settings(&self, platform: &Trigger) -> (TriggerSettings, Vec<String>) {
        let defaults = TriggerSettings::defaults(platform);
        let raw = &self.trigger;
        let mut warnings = Vec::new();
        let mode = match &raw.mode {
            None => defaults.mode,
            Some(value) => value.as_str().and_then(TriggerMode::parse).unwrap_or_else(|| {
                warnings.push(format!(
                    "trigger.mode: unknown value {value} (expected \"hold\" or \"double-tap\"), using the default"
                ));
                defaults.mode
            }),
        };
        let key = match &raw.key {
            None => defaults.key,
            Some(value) => value
                .as_str()
                .and_then(Modifiers::from_trigger_name)
                .unwrap_or_else(|| {
                    warnings.push(format!(
                        "trigger.key: {value} is not a single modifier key, using the default"
                    ));
                    defaults.key
                }),
        };
        let hold_ms = millis(
            &raw.hold_ms,
            "hold-ms",
            HOLD_MS,
            defaults.hold_ms,
            &mut warnings,
        );
        let double_tap_ms = millis(
            &raw.double_tap_ms,
            "double-tap-ms",
            DOUBLE_TAP_MS,
            defaults.double_tap_ms,
            &mut warnings,
        );
        let hide_on_release = match &raw.hide_on_release {
            None => defaults.hide_on_release,
            Some(value) => value.as_bool().unwrap_or_else(|| {
                warnings.push(format!(
                    "trigger.hide-on-release: expected true or false, got {value}; using the default"
                ));
                defaults.hide_on_release
            }),
        };
        let settings = TriggerSettings {
            mode,
            key,
            hold_ms,
            double_tap_ms,
            hide_on_release,
        };
        (settings, warnings)
    }
}

fn millis(
    value: &Option<toml::Value>,
    name: &str,
    range: RangeInclusive<u64>,
    default: u64,
    warnings: &mut Vec<String>,
) -> u64 {
    let Some(value) = value else {
        return default;
    };
    let Some(ms) = value.as_integer() else {
        warnings.push(format!(
            "trigger.{name}: expected a number of milliseconds, got {value}; using the default"
        ));
        return default;
    };
    let clamped = (ms.max(0) as u64).clamp(*range.start(), *range.end());
    if clamped as i64 != ms {
        warnings.push(format!(
            "trigger.{name}: {ms} is outside {}–{}, using {clamped}",
            range.start(),
            range.end()
        ));
    }
    clamped
}

/// Unknown keys are ignored so older versions can read configs written by newer ones.
pub fn parse_config(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| e.to_string())
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
    use keylens_core::{Lang, Modifiers, Trigger};

    fn mac_default() -> Trigger {
        Trigger::HoldModifier {
            key: Modifiers::META,
            delay: Trigger::DEFAULT_HOLD_DELAY,
        }
    }

    fn settings(text: &str) -> (TriggerSettings, Vec<String>) {
        parse_config(text).unwrap().trigger_settings(&mac_default())
    }

    #[test]
    fn missing_trigger_fields_use_the_platform_default() {
        let (s, warnings) = settings("");
        assert_eq!(s, TriggerSettings::defaults(&mac_default()));
        assert_eq!(
            s,
            TriggerSettings {
                mode: TriggerMode::Hold,
                key: Modifiers::META,
                hold_ms: 800,
                double_tap_ms: 350,
                hide_on_release: false,
            }
        );
        assert!(warnings.is_empty());
        assert_eq!(s.trigger(), mac_default());
    }

    #[test]
    fn trigger_fields_are_read() {
        let (s, warnings) = settings(
            "[trigger]\nmode = \"double-tap\"\nkey = \"option\"\nhold-ms = 1200\ndouble-tap-ms = 400\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s.mode, TriggerMode::DoubleTap);
        assert_eq!(s.key, Modifiers::ALT);
        assert_eq!((s.hold_ms, s.double_tap_ms), (1200, 400));
        assert_eq!(
            s.trigger(),
            Trigger::DoubleTap {
                key: Modifiers::ALT,
                interval: std::time::Duration::from_millis(400),
            }
        );
    }

    #[test]
    fn hide_on_release_is_read() {
        let (s, warnings) = settings("[trigger]\nhide-on-release = true\n");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(s.hide_on_release);
    }

    #[test]
    fn a_non_boolean_hide_on_release_falls_back_with_a_warning() {
        let (s, warnings) = settings("[trigger]\nhide-on-release = \"yes\"\n");
        assert!(!s.hide_on_release);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("hide-on-release"));
    }

    #[test]
    fn key_aliases_are_accepted() {
        assert_eq!(
            settings("[trigger]\nkey = \"super\"").0.key,
            Modifiers::META
        );
        assert_eq!(
            settings("[trigger]\nkey = \"Control\"").0.key,
            Modifiers::CTRL
        );
    }

    #[test]
    fn out_of_range_durations_are_clamped_with_a_warning() {
        let (s, warnings) = settings("[trigger]\nhold-ms = 10\ndouble-tap-ms = 99999\n");
        assert_eq!((s.hold_ms, s.double_tap_ms), (300, 600));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("hold-ms"));
    }

    #[test]
    fn a_bad_field_only_resets_itself() {
        let (s, warnings) = settings(
            "[trigger]\nmode = \"triple\"\nkey = \"fn\"\nhold-ms = \"fast\"\ndouble-tap-ms = 300\n",
        );
        let d = TriggerSettings::defaults(&mac_default());
        assert_eq!((s.mode, s.key, s.hold_ms), (d.mode, d.key, d.hold_ms));
        assert_eq!(s.double_tap_ms, 300);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
    }

    #[test]
    fn unknown_trigger_fields_are_ignored() {
        let (s, warnings) = settings("[trigger]\nfuture = true\nkey = \"shift\"\n");
        assert_eq!(s.key, Modifiers::SHIFT);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_non_table_trigger_makes_the_file_invalid() {
        assert!(parse_config("trigger = 1").is_err());
    }

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
    fn configured_language_wins_over_system() {
        let config = Config {
            language: Some("zh-TW".into()),
            ..Default::default()
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
            ..Default::default()
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
