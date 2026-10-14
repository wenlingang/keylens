//! 数据文件的反序列化结构，只在 crate 内部使用；校验后转换成 `data.rs` 里的类型。

use std::collections::BTreeMap;

use serde::Deserialize;

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
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub linux: Vec<String>,
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
    PerOs(RawPerOs),
}

/// 按值的类型分派，而不是用 `untagged`：表的情况直接反序列化成 `RawPerOs`，
/// 这样 `unknown field` 和 `missing field default` 这类报错能原样传给贡献者。
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
                RawPerOs::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(RawKeys::PerOs)
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
pub(crate) struct RawPerOs {
    pub default: RawSeq,
    pub macos: Option<RawSeq>,
    pub windows: Option<RawSeq>,
    pub linux: Option<RawSeq>,
}
