//! Language detection, fallback rules and multilingual text.

use std::collections::BTreeMap;

/// Content language. The order is also the output order of `Localized::all`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lang {
    En,
    ZhHans,
    ZhHant,
    Ja,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::En, Lang::ZhHans, Lang::ZhHant, Lang::Ja];

    /// Key in the community data `i18n` table. English lives in `title` and has no key.
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

    /// Lookup order when a translation is missing; always ends at English.
    pub fn fallbacks(self) -> &'static [Lang] {
        match self {
            Lang::En => &[Lang::En],
            Lang::ZhHans => &[Lang::ZhHans, Lang::En],
            Lang::ZhHant => &[Lang::ZhHant, Lang::ZhHans, Lang::En],
            Lang::Ja => &[Lang::Ja, Lang::En],
        }
    }
}

/// `lang` selects the community data translation; `ui` is the UI language code passed to rust-i18n (consistent with gpui-kit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locale {
    pub lang: Lang,
    pub ui: &'static str,
}

const ENGLISH: Locale = Locale {
    lang: Lang::En,
    ui: "en",
};

/// Maps language tags from the system or config (`zh-Hant-HK`, `zh_TW`, `ja-JP`, etc.) to a supported language.
pub fn resolve(tag: &str) -> Locale {
    let tag = tag.trim().replace('_', "-").to_ascii_lowercase();
    let parts: Vec<&str> = tag.split('-').collect();
    let has = |part: &str| parts.iter().skip(1).any(|p| *p == part);
    match parts[0] {
        "zh" if has("hans") => Locale {
            lang: Lang::ZhHans,
            ui: "zh-CN",
        },
        "zh" if has("hk") => Locale {
            lang: Lang::ZhHant,
            ui: "zh-HK",
        },
        "zh" if has("hant") || has("tw") || has("mo") => Locale {
            lang: Lang::ZhHant,
            ui: "zh-TW",
        },
        "zh" => Locale {
            lang: Lang::ZhHans,
            ui: "zh-CN",
        },
        "ja" => Locale {
            lang: Lang::Ja,
            ui: "ja",
        },
        _ => ENGLISH,
    }
}

/// Finds the first supported language in the user's preferred language list; English itself counts as supported.
pub fn resolve_preferred<'a>(tags: impl IntoIterator<Item = &'a str>) -> Locale {
    for tag in tags {
        let locale = resolve(tag);
        if locale.lang != Lang::En || tag.trim().to_ascii_lowercase().starts_with("en") {
            return locale;
        }
    }
    ENGLISH
}

/// Text with translations: English is required, other languages are optional.
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

    /// English first, then all translations in `Lang` order.
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
        assert_eq!(
            resolve_preferred(["fr-FR", "ja-JP", "en-US"]).lang,
            Lang::Ja
        );
        assert_eq!(resolve_preferred(["en-GB", "zh-CN"]).lang, Lang::En);
        assert_eq!(resolve_preferred(["de-DE"]).lang, Lang::En);
        assert_eq!(resolve_preferred(std::iter::empty()).lang, Lang::En);
    }

    #[test]
    fn localized_text_falls_back() {
        let mut text = Localized {
            en: "Save".into(),
            translations: BTreeMap::new(),
        };
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
