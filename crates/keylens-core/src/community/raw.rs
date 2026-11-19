//! Deserialization structs for data files, used only inside the crate; converted to the types in `data.rs` after validation.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::IgnoredAny;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFile {
    pub schema: u32,
    pub app: RawApp,
    #[serde(default)]
    pub group: Vec<RawGroup>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawApp {
    pub name: String,
    #[serde(rename = "match")]
    pub matches: RawMatch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMatch {
    #[serde(default)]
    pub macos: Vec<String>,
    /// Legacy key from when Windows was supported; kept so old files still load, value ignored.
    #[serde(default, rename = "windows")]
    _windows: Option<IgnoredAny>,
    /// Legacy key from when Linux was supported; kept so old files still load, value ignored.
    #[serde(default, rename = "linux")]
    _linux: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGroup {
    pub title: String,
    #[serde(default)]
    pub i18n: BTreeMap<String, String>,
    #[serde(default)]
    pub shortcut: Vec<RawShortcut>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawShortcut {
    pub title: String,
    #[serde(default)]
    pub i18n: BTreeMap<String, String>,
    pub keys: RawKeys,
}

pub(crate) enum RawKeys {
    Same(RawSeq),
    Table(RawKeyTable),
}

/// Dispatches on the value's type instead of using `untagged`: the table case deserializes directly into `RawKeyTable`,
/// so errors like `unknown field` and `missing field default` reach contributors unchanged.
impl<'de> Deserialize<'de> for RawKeys {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeysVisitor;

        impl<'de> serde::de::Visitor<'de> for KeysVisitor {
            type Value = RawKeys;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string, array or table of key combinations")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<RawKeys, E> {
                Ok(RawKeys::Same(RawSeq::One(value.to_string())))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, seq: A) -> Result<RawKeys, A::Error> {
                let parts =
                    Vec::<String>::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))?;
                Ok(RawKeys::Same(RawSeq::Many(parts)))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<RawKeys, A::Error> {
                RawKeyTable::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(RawKeys::Table)
            }
        }

        deserializer.deserialize_any(KeysVisitor)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum RawSeq {
    One(String),
    Many(Vec<String>),
}

impl RawSeq {
    pub fn parts(&self) -> Vec<&str> {
        match self {
            RawSeq::One(s) => vec![s.as_str()],
            RawSeq::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
/// The table form `keys = { default = ..., macos = ... }`, from when shortcuts could differ per
/// platform. Still accepted so existing files keep their meaning: `macos` overrides `default`.
pub(crate) struct RawKeyTable {
    pub default: RawSeq,
    pub macos: Option<RawSeq>,
    /// Legacy key from when Windows was supported; kept so old files still load, value ignored.
    #[serde(default, rename = "windows")]
    _windows: Option<IgnoredAny>,
    /// Legacy key from when Linux was supported; kept so old files still load, value ignored.
    #[serde(default, rename = "linux")]
    _linux: Option<IgnoredAny>,
}

impl RawKeys {
    /// The key sequence that applies on macOS.
    pub fn seq(&self) -> &RawSeq {
        match self {
            RawKeys::Same(seq) => seq,
            RawKeys::Table(table) => table.macos.as_ref().unwrap_or(&table.default),
        }
    }
}
