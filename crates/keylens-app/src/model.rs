//! The panel's UI state: grouping into columns, search results, selection and keyboard navigation. Pure logic, no gpui dependency.

use std::ops::Range;

use keylens_core::{AppInfo, AppShortcuts, Entry, EntrySource, Os, SourceError, filter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelStatus {
    Loading,
    Ready,
    NoPermission,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Header {
        title: String,
        community: bool,
    },
    Item {
        entry: usize,
        highlight: Option<Range<usize>>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecKind {
    Menu,
    Keys,
    Disabled,
}

/// Distributes groups into at most `columns` columns in order, keeping column heights close. `heights` is the row count of each group.
pub fn layout_columns(heights: &[usize], columns: usize) -> Vec<Vec<usize>> {
    let columns = columns.max(1);
    let total: usize = heights.iter().sum();
    let target = total.div_ceil(columns);
    let mut result: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut height = 0;
    for (i, &h) in heights.iter().enumerate() {
        if !current.is_empty() && height + h > target && result.len() + 1 < columns {
            result.push(std::mem::take(&mut current));
            height = 0;
        }
        current.push(i);
        height += h;
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

/// The new scroll offset needed to bring the row `[top, bottom)` into the viewport; `None` if it is already visible.
pub fn scroll_target(top: f32, bottom: f32, scrolled: f32, viewport: f32) -> Option<f32> {
    if top < scrolled {
        Some(top)
    } else if bottom > scrolled + viewport {
        Some(bottom - viewport)
    } else {
        None
    }
}

struct GroupRows {
    title: String,
    community: bool,
    entries: Vec<usize>,
}

pub struct PanelModel {
    os: Os,
    status: PanelStatus,
    app: Option<AppInfo>,
    entries: Vec<Entry>,
    groups: Vec<GroupRows>,
    warnings: Vec<SourceError>,
    query: String,
    columns: usize,
    layout: Vec<Vec<Row>>,
    selected: Option<(usize, usize)>,
}

impl PanelModel {
    pub fn new(os: Os) -> Self {
        Self {
            os,
            status: PanelStatus::Loading,
            app: None,
            entries: Vec::new(),
            groups: Vec::new(),
            warnings: Vec::new(),
            query: String::new(),
            columns: 1,
            layout: Vec::new(),
            selected: None,
        }
    }

    pub fn status(&self) -> PanelStatus {
        self.status
    }

    pub fn set_status(&mut self, status: PanelStatus) {
        self.status = status;
    }

    pub fn app(&self) -> Option<&AppInfo> {
        self.app.as_ref()
    }

    pub fn os(&self) -> Os {
        self.os
    }

    /// Starts showing an app that has no data yet.
    pub fn set_loading(&mut self, app: AppInfo) {
        self.app = Some(app);
        self.entries.clear();
        self.groups.clear();
        self.warnings.clear();
        self.status = PanelStatus::Loading;
        self.rebuild(None);
    }

    pub fn set_data(&mut self, data: AppShortcuts) {
        let keep = self.selection_key();
        self.app = Some(data.app);
        self.warnings = data.warnings;
        self.entries.clear();
        self.groups.clear();
        for group in data.groups {
            let start = self.entries.len();
            self.entries.extend(group.entries);
            self.groups.push(GroupRows {
                title: group.title,
                community: group.community,
                entries: (start..self.entries.len()).collect(),
            });
        }
        self.status = PanelStatus::Ready;
        self.rebuild(keep);
    }

    pub fn warnings(&self) -> &[SourceError] {
        &self.warnings
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, query: &str) {
        if self.query == query {
            return;
        }
        self.query = query.to_string();
        self.rebuild(None);
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn set_columns(&mut self, columns: usize) {
        let columns = columns.max(1);
        if self.columns == columns {
            return;
        }
        self.columns = columns;
        let keep = self.selection_key();
        self.rebuild(keep);
    }

    pub fn layout(&self) -> &[Vec<Row>] {
        &self.layout
    }

    pub fn entry(&self, index: usize) -> &Entry {
        &self.entries[index]
    }

    pub fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn item_count(&self) -> usize {
        self.layout
            .iter()
            .flatten()
            .filter(|r| matches!(r, Row::Item { .. }))
            .count()
    }

    pub fn selected(&self) -> Option<(usize, usize)> {
        self.selected
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        let (c, r) = self.selected?;
        match &self.layout[c][r] {
            Row::Item { entry, .. } => Some(&self.entries[*entry]),
            Row::Header { .. } => None,
        }
    }

    /// Selects an entry (mouse click).
    pub fn select_entry(&mut self, entry: usize) {
        if let Some(pos) = self.position_of(|e| e == entry) {
            self.selected = Some(pos);
        }
    }

    /// Select the first entry (top of the first column).
    pub fn select_first(&mut self) {
        self.selected = self.position_of(|_| true);
    }

    pub fn move_down(&mut self) {
        self.step(1);
    }

    pub fn move_up(&mut self) {
        self.step(-1);
    }

    pub fn move_right(&mut self) {
        self.jump_column(1);
    }

    pub fn move_left(&mut self) {
        self.jump_column(-1);
    }

    pub fn breadcrumb(&self, entry: &Entry) -> String {
        std::iter::once(entry.group.as_str())
            .chain(entry.path.iter().map(String::as_str))
            .chain(std::iter::once(entry.title.as_str()))
            .collect::<Vec<_>>()
            .join(" › ")
    }

    pub fn exec_kind(&self, entry: &Entry) -> ExecKind {
        if !entry.enabled {
            ExecKind::Disabled
        } else if entry.source() == EntrySource::Menu {
            ExecKind::Menu
        } else {
            ExecKind::Keys
        }
    }

    // ---- Internals ----

    /// Key used to find the selected entry again after a refresh: group, path, title.
    fn selection_key(&self) -> Option<(String, Vec<String>, String)> {
        self.selected_entry()
            .map(|e| (e.group.clone(), e.path.clone(), e.title.clone()))
    }

    fn rebuild(&mut self, keep: Option<(String, Vec<String>, String)>) {
        self.layout = if self.query.trim().is_empty() {
            let heights: Vec<usize> = self.groups.iter().map(|g| g.entries.len() + 1).collect();
            layout_columns(&heights, self.columns)
                .into_iter()
                .map(|column| {
                    column
                        .into_iter()
                        .flat_map(|g| {
                            let group = &self.groups[g];
                            std::iter::once(Row::Header {
                                title: group.title.clone(),
                                community: group.community,
                            })
                            .chain(group.entries.iter().map(
                                |&entry| Row::Item {
                                    entry,
                                    highlight: None,
                                },
                            ))
                        })
                        .collect()
                })
                .collect()
        } else {
            let rows: Vec<Row> = filter(&self.entries, &self.query, self.os)
                .into_iter()
                .map(|m| Row::Item {
                    entry: m.index,
                    highlight: m.highlight,
                })
                .collect();
            if rows.is_empty() {
                Vec::new()
            } else {
                vec![rows]
            }
        };
        let kept = keep.and_then(|(group, path, title)| {
            let entries = &self.entries;
            self.position_of(|e| {
                entries[e].group == group && entries[e].path == path && entries[e].title == title
            })
        });
        self.selected = kept.or_else(|| self.position_of(|_| true));
    }

    fn position_of(&self, mut pred: impl FnMut(usize) -> bool) -> Option<(usize, usize)> {
        self.layout.iter().enumerate().find_map(|(c, column)| {
            column
                .iter()
                .position(|row| matches!(row, Row::Item { entry, .. } if pred(*entry)))
                .map(|r| (c, r))
        })
    }

    fn item_rows(&self, column: usize) -> Vec<usize> {
        self.layout[column]
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, Row::Item { .. }))
            .map(|(r, _)| r)
            .collect()
    }

    fn step(&mut self, delta: isize) {
        let Some((c, r)) = self.selected else { return };
        let rows = self.item_rows(c);
        let i = rows.iter().position(|&x| x == r).unwrap_or(0) as isize + delta;
        if i >= 0 && (i as usize) < rows.len() {
            self.selected = Some((c, rows[i as usize]));
            return;
        }
        let mut column = c as isize + delta;
        while column >= 0 && (column as usize) < self.layout.len() {
            let rows = self.item_rows(column as usize);
            let pick = if delta > 0 { rows.first() } else { rows.last() };
            if let Some(&row) = pick {
                self.selected = Some((column as usize, row));
                return;
            }
            column += delta;
        }
    }

    fn jump_column(&mut self, delta: isize) {
        let Some((c, r)) = self.selected else { return };
        let mut column = c as isize + delta;
        while column >= 0 && (column as usize) < self.layout.len() {
            let rows = self.item_rows(column as usize);
            if let Some(&row) = rows
                .iter()
                .min_by_key(|&&x| (x as isize - r as isize).unsigned_abs())
            {
                self.selected = Some((column as usize, row));
                return;
            }
            column += delta;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::{EntryId, Group, Shortcut};

    fn entry(source: EntrySource, index: u32, group: &str, title: &str, keys: &str) -> Entry {
        Entry {
            id: EntryId {
                source,
                snapshot: 1,
                index,
            },
            group: group.into(),
            path: vec![],
            title: title.into(),
            search_terms: vec![],
            shortcut: Shortcut::parse_seq(&[keys], Os::MacOS).unwrap(),
            enabled: true,
        }
    }

    fn group(title: &str, community: bool, entries: Vec<Entry>) -> Group {
        Group {
            title: title.into(),
            community,
            entries,
        }
    }

    fn data() -> AppShortcuts {
        let m = EntrySource::Menu;
        AppShortcuts {
            app: AppInfo {
                pid: 1,
                name: "Demo".into(),
                ids: vec![],
            },
            groups: vec![
                group(
                    "文件",
                    false,
                    vec![
                        entry(m, 0, "文件", "新建", "Mod+N"),
                        entry(m, 1, "文件", "打开", "Mod+O"),
                        entry(m, 2, "文件", "保存", "Mod+S"),
                    ],
                ),
                group(
                    "编辑",
                    false,
                    vec![
                        entry(m, 3, "编辑", "拷贝", "Mod+C"),
                        entry(m, 4, "编辑", "粘贴", "Mod+V"),
                    ],
                ),
                group(
                    "General",
                    true,
                    vec![entry(
                        EntrySource::Community,
                        0,
                        "General",
                        "命令面板",
                        "Mod+Shift+P",
                    )],
                ),
            ],
            warnings: vec![],
        }
    }

    fn ready(columns: usize) -> PanelModel {
        let mut m = PanelModel::new(Os::MacOS);
        m.set_columns(columns);
        m.set_data(data());
        m
    }

    fn selected_title(m: &PanelModel) -> &str {
        &m.selected_entry().unwrap().title
    }

    #[test]
    fn layout_keeps_order_and_balances_columns() {
        assert_eq!(layout_columns(&[4, 4, 4, 4], 2), [vec![0, 1], vec![2, 3]]);
        assert_eq!(layout_columns(&[10, 2, 2], 2), [vec![0], vec![1, 2]]);
        assert_eq!(layout_columns(&[3, 3], 4), [vec![0], vec![1]]);
        assert_eq!(layout_columns(&[3, 3], 1), [vec![0, 1]]);
        assert!(layout_columns(&[], 3).is_empty());
    }

    #[test]
    fn data_is_shown_in_columns_with_headers() {
        let m = ready(2);
        assert_eq!(m.status(), PanelStatus::Ready);
        assert_eq!(m.layout().len(), 2);
        assert_eq!(
            m.layout()[0][0],
            Row::Header {
                title: "文件".into(),
                community: false
            }
        );
        assert_eq!(m.item_count(), 6);
        assert_eq!(selected_title(&m), "新建");
    }

    #[test]
    fn down_and_up_skip_headers_and_cross_columns() {
        let mut m = ready(2);
        m.move_down();
        m.move_down();
        assert_eq!(selected_title(&m), "保存");
        m.move_down();
        assert_eq!(selected_title(&m), "拷贝");
        m.move_up();
        assert_eq!(selected_title(&m), "保存");
        m.move_up();
        m.move_up();
        m.move_up();
        assert_eq!(selected_title(&m), "新建");
    }

    #[test]
    fn left_and_right_jump_to_the_nearest_row() {
        let mut m = ready(2);
        m.move_down(); // Open (column 0, row 2)
        m.move_right();
        assert_eq!(selected_title(&m), "粘贴"); // Column 1, row 2, same row number
        m.move_down(); // Command Palette (column 1, row 4, skipping the General heading)
        assert_eq!(selected_title(&m), "命令面板");
        m.move_left();
        assert_eq!(selected_title(&m), "保存");
        m.move_left();
        assert_eq!(selected_title(&m), "保存");
    }

    #[test]
    fn search_shows_one_column_with_highlights() {
        let mut m = ready(2);
        m.set_query("面板");
        assert_eq!(m.layout().len(), 1);
        assert_eq!(m.item_count(), 1);
        let Row::Item { highlight, .. } = &m.layout()[0][0] else {
            panic!("expected an entry row")
        };
        assert!(highlight.is_some());
        assert_eq!(selected_title(&m), "命令面板");
        m.set_query("不存在");
        assert_eq!(m.item_count(), 0);
        assert!(m.selected().is_none());
        m.set_query("");
        assert_eq!(m.layout().len(), 2);
    }

    #[test]
    fn selection_survives_refresh_and_column_changes() {
        let mut m = ready(2);
        m.move_down();
        m.move_down(); // Save
        m.set_columns(1);
        assert_eq!(selected_title(&m), "保存");
        let mut refreshed = data();
        refreshed.groups[0].entries.insert(
            0,
            entry(EntrySource::Menu, 9, "文件", "新建窗口", "Mod+Shift+N"),
        );
        m.set_data(refreshed);
        assert_eq!(selected_title(&m), "保存");
    }

    #[test]
    fn loading_and_permission_states() {
        let mut m = PanelModel::new(Os::MacOS);
        m.set_loading(AppInfo {
            pid: 3,
            name: "X".into(),
            ids: vec![],
        });
        assert_eq!(m.status(), PanelStatus::Loading);
        assert_eq!(m.app().unwrap().pid, 3);
        assert!(!m.has_entries());
        m.set_status(PanelStatus::NoPermission);
        assert_eq!(m.status(), PanelStatus::NoPermission);
    }

    #[test]
    fn breadcrumb_and_exec_kind() {
        let m = ready(1);
        let mut e = entry(EntrySource::Menu, 0, "格式", "显示字体", "Mod+T");
        e.path = vec!["字体".into()];
        assert_eq!(m.breadcrumb(&e), "格式 › 字体 › 显示字体");
        assert_eq!(m.exec_kind(&e), ExecKind::Menu);
        e.enabled = false;
        assert_eq!(m.exec_kind(&e), ExecKind::Disabled);
        let c = entry(
            EntrySource::Community,
            0,
            "General",
            "命令面板",
            "Mod+Shift+P",
        );
        assert_eq!(m.exec_kind(&c), ExecKind::Keys);
    }

    #[test]
    fn scroll_target_only_moves_when_needed() {
        assert_eq!(scroll_target(100.0, 126.0, 0.0, 300.0), None);
        assert_eq!(scroll_target(400.0, 426.0, 0.0, 300.0), Some(126.0));
        assert_eq!(scroll_target(50.0, 76.0, 200.0, 300.0), Some(50.0));
    }
}
