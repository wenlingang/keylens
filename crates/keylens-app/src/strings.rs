//! List of UI string keys. Add new strings here too; the completeness test checks that all 5 languages have a translation.

pub const KEYS: &[&str] = &[
    "Panel.search_placeholder",
    "Panel.count",
    "Panel.execute",
    "Panel.close",
    "Panel.how_menu",
    "Panel.how_keys",
    "Panel.how_disabled",
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
    "Panel.add_shortcuts",
    "Panel.edit_shortcuts",
    "Panel.settings",
    "Panel.unreadable_title",
    "Panel.unreadable_body",
    "Tray.permission_ok",
    "Tray.permission_missing",
    "Tray.open_data",
    "Tray.edit_current",
    "Tray.data_errors",
    "Tray.settings",
    "Tray.check_updates",
    "Tray.updating",
    "Tray.quit",
    "Settings.title",
    "Settings.trigger_group",
    "Settings.mode",
    "Settings.mode_hold",
    "Settings.mode_double_tap",
    "Settings.key",
    "Settings.hold_ms",
    "Settings.double_tap_ms",
    "Settings.hold_note",
    "Settings.double_tap_note",
    "Settings.ui_group",
    "Settings.language",
    "Settings.language_system",
    "Settings.reset",
    "Settings.reset_all",
    "Settings.invalid",
    "Settings.write_failed",
    "Settings.open_file",
    "Editor.title",
    "Editor.empty",
    "Editor.add",
    "Editor.edit",
    "Editor.delete",
    "Editor.title_label",
    "Editor.keys_label",
    "Editor.group_label",
    "Editor.new_group",
    "Editor.default_group",
    "Editor.recording",
    "Editor.click_to_record",
    "Editor.manual",
    "Editor.per_platform",
    "Editor.save",
    "Editor.cancel",
    "Editor.title_required",
    "Editor.invalid_keys",
    "Editor.duplicate",
    "Editor.deleted",
    "Editor.undo",
    "Editor.open_in_editor",
    "Editor.invalid",
    "Editor.write_failed",
    "Editor.group_required",
    "Editor.change_failed",
    "Editor.stale",
    "Update.available_title",
    "Update.available_body",
    "Update.install",
    "Update.later",
    "Update.ok",
    "Update.up_to_date_title",
    "Update.up_to_date_body",
    "Update.installed_title",
    "Update.installed_body",
    "Update.check_failed",
    "Update.install_failed",
    "Update.err_network",
    "Update.err_manifest",
    "Update.err_checksum",
    "Update.err_not_in_app",
    "Update.err_translocated",
    "Update.err_unsigned",
    "Update.err_signature",
    "Update.err_replace",
    "Update.err_other",
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
        assert_eq!(t!("Panel.count", locale = "en", count = 3), "Shortcuts: 3");
        assert!(t!("Panel.no_match", locale = "zh-CN", query = "abc").contains("abc"));
    }
}
