//! 快捷键条目和应用信息。

use crate::keys::Shortcut;

/// 条目来源，排列顺序就是合并时的优先级（菜单最高）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntrySource {
    Menu,
    User,
    Community,
}

/// 条目在本次读取结果中的唯一编号；平台层用它找回菜单元素。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntryId {
    pub source: EntrySource,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: EntryId,
    /// 顶层菜单名或社区数据分组名（已本地化）。
    pub group: String,
    /// 顶层菜单和条目之间的子菜单路径。
    pub path: Vec<String>,
    /// 已本地化的显示标题。
    pub title: String,
    /// 除标题外也参与搜索的文字，例如其他语言的标题。
    pub search_terms: Vec<String>,
    pub shortcut: Shortcut,
    pub enabled: bool,
}

impl Entry {
    pub fn source(&self) -> EntrySource {
        self.id.source
    }
}

/// 前台应用。`ids` 是当前平台上用来识别应用的标识，macOS 上是 bundle ID。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub pid: i32,
    pub name: String,
    pub ids: Vec<String>,
}

/// 界面上的一个分组。`community` 为 true 时标题旁显示 ◇。
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub title: String,
    pub community: bool,
    pub entries: Vec<Entry>,
}
