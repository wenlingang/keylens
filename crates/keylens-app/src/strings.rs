//! List of UI string keys. Add new strings here too; the completeness test checks that all 5 languages have a translation.

pub const KEYS: &[&str] = &[
    "Panel.search_placeholder",
    "Panel.count",
    "Panel.execute",
    "Panel.close",
    "Panel.how_menu",
    "Panel.how_keys",
    "Panel.how_disabled",
    "Panel.community",
    "Panel.loading",
    "Panel.no_match",
    "Panel.no_match_hint",
    "Panel.empty_title",
    "Panel.empty_body",
    "Panel.menu_unreadable",
    "Panel.perm_title",
    "Panel.perm_body",
    "Panel.perm_hint",
    "Panel.open_settings",
    "Tray.permission_ok",
    "Tray.permission_missing",
    "Tray.open_data",
    "Tray.data_errors",
    "Tray.quit",
];

pub const LOCALES: &[&str] = &["en", "zh-CN", "zh-TW", "zh-HK", "ja"];

#[cfg(test)]
mod tests {
    use super::*;
    use rust_i18n::t;

    #[test]
    fn every_key_is_translated_in_every_locale() {
        // Query the backend directly: `t!` falls back to English, so it would hide missing translations.
        let backend = crate::_rust_i18n_backend();
        let mut missing = Vec::new();
        for locale in LOCALES {
            for key in KEYS {
                let text = backend.translate(locale, key);
                if text.as_deref().is_none_or(|t| t.trim().is_empty()) {
                    missing.push(format!("{locale}: {key}"));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "missing translations:\n{}",
            missing.join("\n")
        );
    }

    #[test]
    fn interpolation_works() {
        assert_eq!(t!("Panel.count", locale = "en", count = 3), "3 shortcuts");
        assert!(t!("Panel.no_match", locale = "zh-CN", query = "abc").contains("abc"));
    }
}
