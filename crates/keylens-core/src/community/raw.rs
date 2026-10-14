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

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum RawKeys {
    Same(RawSeq),
    PerOs(RawPerOs),
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
