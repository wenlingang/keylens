//! 语言识别、回退规则和多语言文字。

use std::collections::BTreeMap;

/// 内容语言。顺序也是 `Localized::all` 的输出顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lang {
    En,
    ZhHans,
    ZhHant,
    Ja,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::En, Lang::ZhHans, Lang::ZhHant, Lang::Ja];

    /// 社区数据 `i18n` 表里的键。英文写在 `title`，没有键。
    pub fn data_key(self) -> Option<&'static str> {
        match self {
            Lang::En => None,
            Lang::ZhHans => Some("zh-Hans"),
            Lang::ZhHant => Some("zh-Hant"),
            Lang::Ja => Some("ja"),
        }
    }

    pub fn from_data_key(key: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.data_key() == Some(key))
    }

    /// 缺少翻译时的查找顺序，最后都是英文。
    pub fn fallbacks(self) -> &'static [Lang] {
        match self {
            Lang::En => &[Lang::En],
            Lang::ZhHans => &[Lang::ZhHans, Lang::En],
            Lang::ZhHant => &[Lang::ZhHant, Lang::ZhHans, Lang::En],
            Lang::Ja => &[Lang::Ja, Lang::En],
        }
    }
}

/// `lang` 用来选社区数据的翻译，`ui` 是传给 rust-i18n 的界面语言代码（与 gpui-kit 一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locale {
    pub lang: Lang,
    pub ui: &'static str,
}

const ENGLISH: Locale = Locale { lang: Lang::En, ui: "en" };

/// 把系统或配置里的语言标签（`zh-Hant-HK`、`zh_TW`、`ja-JP` 等）映射到支持的语言。
pub fn resolve(tag: &str) -> Locale {
    let tag = tag.trim().replace('_', "-").to_ascii_lowercase();
    let parts: Vec<&str> = tag.split('-').collect();
    let has = |part: &str| parts.iter().skip(1).any(|p| *p == part);
    match parts[0] {
        "zh" if has("hans") => Locale { lang: Lang::ZhHans, ui: "zh-CN" },
        "zh" if has("hk") => Locale { lang: Lang::ZhHant, ui: "zh-HK" },
        "zh" if has("hant") || has("tw") || has("mo") => Locale { lang: Lang::ZhHant, ui: "zh-TW" },
        "zh" => Locale { lang: Lang::ZhHans, ui: "zh-CN" },
        "ja" => Locale { lang: Lang::Ja, ui: "ja" },
        _ => ENGLISH,
    }
}

/// 在用户的首选语言列表里找第一个支持的语言；英文本身也算支持。
pub fn resolve_preferred<'a>(tags: impl IntoIterator<Item = &'a str>) -> Locale {
    for tag in tags {
        let locale = resolve(tag);
        if locale.lang != Lang::En || tag.trim().to_ascii_lowercase().starts_with("en") {
            return locale;
        }
    }
    ENGLISH
}

/// 带翻译的文字：英文必有，其他语言可选。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Localized {
    pub en: String,
    pub translations: BTreeMap<Lang, String>,
}

impl Localized {
    pub fn get(&self, lang: Lang) -> &str {
        for candidate in lang.fallbacks() {
            if *candidate == Lang::En {
                return &self.en;
            }
            if let Some(text) = self.translations.get(candidate) {
                return text;
            }
        }
        &self.en
    }

    /// 英文在前，之后按 `Lang` 的顺序列出全部翻译。
    pub fn all(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.en.as_str()).chain(self.translations.values().map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_system_language_tags() {
        let cases = [
            ("zh-Hans-CN", Lang::ZhHans, "zh-CN"),
            ("zh-CN", Lang::ZhHans, "zh-CN"),
            ("zh-SG", Lang::ZhHans, "zh-CN"),
            ("zh_TW", Lang::ZhHant, "zh-TW"),
            ("zh-Hant-TW", Lang::ZhHant, "zh-TW"),
            ("zh-Hant-HK", Lang::ZhHant, "zh-HK"),
            ("zh-HK", Lang::ZhHant, "zh-HK"),
            ("zh-MO", Lang::ZhHant, "zh-TW"),
            ("zh-Hans-HK", Lang::ZhHans, "zh-CN"),
            ("ja-JP", Lang::Ja, "ja"),
            ("en-US", Lang::En, "en"),
            ("fr-FR", Lang::En, "en"),
            ("", Lang::En, "en"),
        ];
        for (tag, lang, ui) in cases {
            assert_eq!(resolve(tag), Locale { lang, ui }, "{tag}");
        }
    }

    #[test]
    fn preferred_list_picks_first_supported() {
        assert_eq!(resolve_preferred(["fr-FR", "ja-JP", "en-US"]).lang, Lang::Ja);
        assert_eq!(resolve_preferred(["en-GB", "zh-CN"]).lang, Lang::En);
        assert_eq!(resolve_preferred(["de-DE"]).lang, Lang::En);
        assert_eq!(resolve_preferred(std::iter::empty()).lang, Lang::En);
    }

    #[test]
    fn localized_text_falls_back() {
        let mut text = Localized { en: "Save".into(), translations: BTreeMap::new() };
        text.translations.insert(Lang::ZhHans, "保存".into());
        assert_eq!(text.get(Lang::ZhHans), "保存");
        assert_eq!(text.get(Lang::ZhHant), "保存");
        assert_eq!(text.get(Lang::Ja), "Save");
        assert_eq!(text.get(Lang::En), "Save");
        text.translations.insert(Lang::ZhHant, "儲存".into());
        assert_eq!(text.get(Lang::ZhHant), "儲存");
        assert_eq!(text.all().collect::<Vec<_>>(), ["Save", "保存", "儲存"]);
    }

    #[test]
    fn data_keys_round_trip() {
        for lang in Lang::ALL {
            if let Some(key) = lang.data_key() {
                assert_eq!(Lang::from_data_key(key), Some(lang));
            }
        }
        assert_eq!(Lang::from_data_key("zh-CN"), None);
    }
}
