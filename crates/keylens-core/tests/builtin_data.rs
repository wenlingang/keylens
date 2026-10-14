//! 内置数据校验：CI 必跑。数据有任何错误，构建都会失败。

use std::sync::Arc;

use keylens_core::community::{CommunitySource, builtin};
use keylens_core::{AppInfo, EntrySource, Lang, Os, ShortcutSource};

#[test]
fn all_builtin_files_are_valid() {
    let (catalog, errors) = builtin();
    assert!(errors.is_empty(), "内置数据有错误：\n{errors:#?}");
    assert!(catalog.len() >= 3, "内置数据只有 {} 个应用", catalog.len());
}

#[test]
fn every_builtin_app_has_macos_ids_and_simplified_chinese() {
    let (catalog, _) = builtin();
    let catalog = Arc::new(catalog);
    for (bundle_id, expected_name) in [
        ("com.microsoft.VSCode", "Visual Studio Code"),
        ("com.google.Chrome", "Google Chrome"),
        ("com.tinyspeck.slackmacgap", "Slack"),
    ] {
        let data = catalog
            .find(Os::MacOS, &[bundle_id.to_string()])
            .expect(bundle_id);
        assert_eq!(data.name, expected_name);
        for group in &data.groups {
            assert!(
                group.title.translations.contains_key(&Lang::ZhHans),
                "{expected_name} 分组 {} 缺简体中文",
                group.title.en
            );
            for shortcut in &group.shortcuts {
                assert!(
                    shortcut.title.translations.contains_key(&Lang::ZhHans),
                    "{expected_name} 的 {} 缺简体中文",
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
            "{expected_name} 至少要有 10 条"
        );
    }
}
