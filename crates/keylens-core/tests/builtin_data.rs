//! Built-in data validation: always run in CI. Any data error fails the build.

use std::sync::Arc;

use keylens_core::community::{CommunitySource, builtin};
use keylens_core::{AppInfo, EntrySource, Lang, Os, ShortcutSource};

#[test]
fn all_builtin_files_are_valid() {
    let (catalog, errors) = builtin();
    assert!(errors.is_empty(), "built-in data has errors:\n{errors:#?}");
    assert!(
        catalog.len() >= 9,
        "built-in data has only {} apps",
        catalog.len()
    );
}

#[test]
fn every_builtin_app_has_macos_ids_and_simplified_chinese() {
    let (catalog, _) = builtin();
    let catalog = Arc::new(catalog);
    for (bundle_id, expected_name) in [
        ("com.microsoft.VSCode", "Visual Studio Code"),
        ("com.google.Chrome", "Google Chrome"),
        ("com.tinyspeck.slackmacgap", "Slack"),
        ("com.apple.Safari", "Safari"),
        ("com.apple.finder", "Finder"),
        ("com.apple.Terminal", "Terminal"),
        ("com.googlecode.iterm2", "iTerm2"),
        ("notion.id", "Notion"),
        ("dev.zed.Zed", "Zed"),
    ] {
        let data = catalog
            .find(Os::MacOS, &[bundle_id.to_string()])
            .expect(bundle_id);
        assert_eq!(data.name, expected_name);
        for group in &data.groups {
            assert!(
                group.title.translations.contains_key(&Lang::ZhHans),
                "{expected_name} group {} is missing zh-Hans",
                group.title.en
            );
            for shortcut in &group.shortcuts {
                assert!(
                    shortcut.title.translations.contains_key(&Lang::ZhHans),
                    "{expected_name} {} is missing zh-Hans",
                    shortcut.title.en
                );
            }
        }
        let source = CommunitySource::new(
            catalog.clone(),
            Os::MacOS,
            Lang::ZhHans,
            EntrySource::Community,
        );
        let app = AppInfo {
            pid: 1,
            name: expected_name.into(),
            ids: vec![bundle_id.into()],
        };
        assert!(
            source.load(&app).unwrap().len() >= 10,
            "{expected_name} must have at least 10 entries"
        );
    }
}
