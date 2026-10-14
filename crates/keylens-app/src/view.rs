//! Panel view: renders `PanelModel`, handles keys and mouse, and emits events to the controller.

use std::ops::Range;
use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use keylens_core::{AppInfo, AppShortcuts, Entry, EntrySource, Os, Shortcut};
use rust_i18n::t;

use crate::model::{ExecKind, PanelModel, PanelStatus, Row, scroll_target};

actions!(
    keylens,
    [
        SelectPrev,
        SelectNext,
        SelectLeft,
        SelectRight,
        Execute,
        Dismiss
    ]
);

pub const KEY_CONTEXT: &str = "Keylens";

const COLUMN_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 26.0;
/// Space above and below the columns of rows.
const LIST_PADDING: f32 = 8.0;
const HEADER_HEIGHT: f32 = 24.0;
/// Total height of the top and bottom bars, used to estimate the visible list height.
const CHROME_HEIGHT: f32 = 100.0;

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some(KEY_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Execute, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(KEY_CONTEXT)),
    ]);
}

#[derive(Clone, Debug, PartialEq)]
pub enum PanelEvent {
    Execute(Entry),
    Dismiss,
    OpenSettings,
    /// Create (if missing) and open a user data file for this app.
    AddShortcuts(AppInfo),
}

pub struct PanelView {
    model: PanelModel,
    input: Entity<InputState>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    icon: Option<Arc<Image>>,
    viewport: f32,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for PanelView {}

impl PanelView {
    pub fn new(os: Os, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Panel.search_placeholder").to_string())
        });
        let on_input = cx.subscribe_in(&input, window, |this, input, event: &InputEvent, _, cx| {
            // Enter only goes through the Execute action; only text changes are handled here.
            if let InputEvent::Change = event {
                let query = input.read(cx).value().to_string();
                this.model.set_query(&query);
                this.scroll.set_offset(point(px(0.), px(0.)));
                cx.notify();
            }
        });
        let on_activation = cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                cx.emit(PanelEvent::Dismiss);
            }
        });
        let on_appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
        });
        Self {
            model: PanelModel::new(os),
            input,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            icon: None,
            viewport: 400.0,
            _subscriptions: vec![on_input, on_activation, on_appearance],
        }
    }

    pub fn model(&self) -> &PanelModel {
        &self.model
    }

    pub fn set_loading(&mut self, app: AppInfo, icon: Option<Arc<Image>>, cx: &mut Context<Self>) {
        self.icon = icon;
        self.model.set_loading(app);
        cx.notify();
    }

    pub fn set_data(
        &mut self,
        data: AppShortcuts,
        icon: Option<Arc<Image>>,
        cx: &mut Context<Self>,
    ) {
        if icon.is_some() {
            self.icon = icon;
        }
        self.model.set_data(data);
        cx.notify();
    }

    pub fn set_no_permission(&mut self, cx: &mut Context<Self>) {
        self.model.set_status(PanelStatus::NoPermission);
        cx.notify();
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.model.set_query("");
        // The scroll position goes back to the top, so the selection must too; otherwise
        // Return could run an entry that is out of view.
        self.model.select_first();
        self.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub fn focus_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    // ---- Keys ----

    fn select_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.model.move_up();
        self.scroll_to_selected();
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.model.move_down();
        self.scroll_to_selected();
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.model.move_left();
        self.scroll_to_selected();
        cx.notify();
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.model.move_right();
        self.scroll_to_selected();
        cx.notify();
    }

    fn execute(&mut self, _: &Execute, _: &mut Window, cx: &mut Context<Self>) {
        self.execute_selected(cx);
    }

    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        // Trust the text in the input box: setting text programmatically doesn't always fire a Change event.
        if self.input.read(cx).value().is_empty() {
            cx.emit(PanelEvent::Dismiss);
        } else {
            self.reset(window, cx);
        }
    }

    fn execute_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.model.selected_entry()
            && entry.enabled
        {
            cx.emit(PanelEvent::Execute(entry.clone()));
        }
    }

    fn click(&mut self, entry: usize, count: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.model.select_entry(entry);
        // The mouse-down moved focus to the clicked element; give it back to the search box
        // so typing keeps filtering.
        self.focus_input(window, cx);
        if count >= 2 {
            self.execute_selected(cx);
        }
        cx.notify();
    }

    /// Top and bottom bounds (pixels) of the selected row within its column.
    fn selected_bounds(&self) -> Option<(f32, f32)> {
        let (c, r) = self.model.selected()?;
        let top: f32 = LIST_PADDING
            + self.model.layout()[c][..r]
                .iter()
                .map(|row| {
                    if matches!(row, Row::Header { .. }) {
                        HEADER_HEIGHT
                    } else {
                        ROW_HEIGHT
                    }
                })
                .sum::<f32>();
        Some((top, top + ROW_HEIGHT))
    }

    fn scroll_to_selected(&mut self) {
        let Some((top, bottom)) = self.selected_bounds() else {
            return;
        };
        let scrolled = -f32::from(self.scroll.offset().y);
        // Use the real height of the scroll area once laid out; before that, the estimate.
        let measured = f32::from(self.scroll.bounds().size.height);
        let viewport = if measured > 0.0 {
            measured
        } else {
            self.viewport
        };
        if let Some(target) = scroll_target(top, bottom, scrolled, viewport) {
            self.scroll.set_offset(point(px(0.), px(-target)));
        }
    }

    // ---- Rendering ----

    fn keycaps(&self, shortcut: &Shortcut, cx: &App) -> Div {
        let theme = cx.theme();
        let os = self.model.os();
        let mut caps = h_flex().gap_1().flex_none();
        for (i, chord) in shortcut.chords().iter().enumerate() {
            if i > 0 {
                caps = caps.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("→"),
                );
            }
            for cap in chord.caps(os) {
                caps = caps.child(
                    div()
                        .text_xs()
                        .px_1()
                        .min_w(px(18.))
                        .flex()
                        .justify_center()
                        .rounded(px(4.))
                        .bg(theme.muted)
                        .text_color(theme.foreground)
                        .child(cap),
                );
            }
        }
        caps
    }

    fn title(text: &str, highlight: Option<Range<usize>>, path: Option<String>, cx: &App) -> Div {
        let theme = cx.theme();
        let mut title = h_flex().flex_1().min_w_0().overflow_hidden();
        match highlight {
            Some(range) => {
                title = title
                    .child(text[..range.start].to_string())
                    .child(
                        div()
                            .bg(theme.accent.opacity(0.35))
                            .rounded(px(2.))
                            .child(text[range.clone()].to_string()),
                    )
                    .child(text[range.end..].to_string());
            }
            None => title = title.child(text.to_string()),
        }
        if let Some(path) = path {
            title = title.child(
                div()
                    .ml_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(path),
            );
        }
        title
    }

    fn render_row(&self, column: usize, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        match &self.model.layout()[column][row] {
            Row::Header { title, community } => h_flex()
                .h(px(HEADER_HEIGHT))
                .px_2()
                .gap_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(title.clone())
                .when(*community, |d| {
                    d.child(div().text_color(theme.accent).child("◇"))
                })
                .into_any_element(),
            Row::Item { entry, highlight } => {
                let index = *entry;
                let entry = self.model.entry(index);
                let selected = self.model.selected() == Some((column, row));
                let searching = !self.model.query().is_empty();
                let path = searching.then(|| {
                    std::iter::once(entry.group.clone())
                        .chain(entry.path.iter().cloned())
                        .collect::<Vec<_>>()
                        .join(" › ")
                });
                h_flex()
                    .id(("entry", index))
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .gap_2()
                    .rounded(px(6.))
                    .text_sm()
                    .when(selected, |d| d.bg(theme.list_active))
                    .when(!selected, |d| d.hover(|s| s.bg(theme.muted.opacity(0.5))))
                    .when(!entry.enabled, |d| d.opacity(0.4))
                    .child(Self::title(&entry.title, highlight.clone(), path, cx))
                    .when(entry.source() != EntrySource::Menu, |d| {
                        d.child(div().text_xs().text_color(theme.accent).child("◇"))
                    })
                    .child(self.keycaps(&entry.shortcut, cx))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.click(index, event.click_count(), window, cx);
                    }))
                    .into_any_element()
            }
        }
    }

    fn render_state(
        &self,
        title: String,
        body: Vec<String>,
        action: Option<(String, PanelEvent)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let mut page = v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_2()
            .p_8();
        page = page.child(div().text_base().text_color(theme.foreground).child(title));
        for line in body {
            page = page.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(line),
            );
        }
        if let Some((label, event)) = action {
            page = page.child(
                div()
                    .id("state-action")
                    .mt_3()
                    .px_3()
                    .py_1()
                    .rounded(px(6.))
                    .bg(theme.accent)
                    .text_color(theme.accent_foreground)
                    .child(label)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(event.clone()))),
            );
        }
        page.into_any_element()
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.model.status() {
            PanelStatus::NoPermission => {
                return self.render_state(
                    t!("Panel.perm_title").to_string(),
                    vec![
                        t!("Panel.perm_body").to_string(),
                        t!("Panel.perm_hint").to_string(),
                    ],
                    Some((
                        t!("Panel.open_settings").to_string(),
                        PanelEvent::OpenSettings,
                    )),
                    cx,
                );
            }
            PanelStatus::Loading if !self.model.has_entries() => {
                return self.render_state(t!("Panel.loading").to_string(), vec![], None, cx);
            }
            _ => {}
        }
        if !self.model.has_entries() {
            if !self.model.warnings().is_empty() {
                // The menu read failed and there is no community data either.
                return self.render_state(
                    t!("Panel.unreadable_title").to_string(),
                    vec![t!("Panel.unreadable_body").to_string()],
                    None,
                    cx,
                );
            }
            let add = self.model.app().map(|app| {
                (
                    t!("Panel.add_shortcuts").to_string(),
                    PanelEvent::AddShortcuts(app.clone()),
                )
            });
            return self.render_state(
                t!("Panel.empty_title").to_string(),
                vec![t!("Panel.empty_body").to_string()],
                add,
                cx,
            );
        }
        if self.model.item_count() == 0 {
            return self.render_state(
                t!("Panel.no_match", query = self.model.query()).to_string(),
                vec![t!("Panel.no_match_hint").to_string()],
                None,
                cx,
            );
        }
        let mut columns = h_flex()
            .items_start()
            .gap_2()
            .px_2()
            .pt(px(LIST_PADDING))
            .pb(px(LIST_PADDING));
        for (c, column) in self.model.layout().iter().enumerate() {
            let mut col = v_flex().flex_1().min_w_0();
            for r in 0..column.len() {
                col = col.child(self.render_row(c, r, cx));
            }
            columns = columns.child(col);
        }
        div()
            .id("body")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(columns)
            .into_any_element()
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (crumb, how) = match self.model.selected_entry() {
            Some(entry) => {
                let how = match self.model.exec_kind(entry) {
                    ExecKind::Menu => t!("Panel.how_menu"),
                    ExecKind::Keys => t!("Panel.how_keys"),
                    ExecKind::Disabled => t!("Panel.how_disabled"),
                };
                (self.model.breadcrumb(entry), how.to_string())
            }
            None => (String::new(), String::new()),
        };
        h_flex()
            .h(px(36.))
            .px_4()
            .gap_3()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(div().text_color(theme.foreground).child(crumb))
                    .child(how),
            )
            .child(format!("↩ {}", t!("Panel.execute")))
            .child(format!("esc {}", t!("Panel.close")))
            .into_any_element()
    }
}

impl Render for PanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = window.bounds();
        let width = f32::from(bounds.size.width);
        self.viewport = (f32::from(bounds.size.height) - CHROME_HEIGHT).max(ROW_HEIGHT);
        self.model
            .set_columns(((width - 16.0) / COLUMN_WIDTH).floor().max(1.0) as usize);

        let theme = cx.theme();
        let app_name = self.model.app().map(|a| a.name.clone()).unwrap_or_default();
        let count = self.model.item_count();
        let unreadable =
            self.model.status() == PanelStatus::Ready && !self.model.warnings().is_empty();

        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_prev))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::execute))
            .on_action(cx.listener(Self::dismiss))
            .size_full()
            .bg(theme.popover)
            .text_color(theme.foreground)
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .h(px(56.))
                    .px_4()
                    .gap_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .when_some(self.icon.clone(), |d, icon| {
                        d.child(img(icon).size(px(28.)))
                    })
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(app_name),
                    )
                    .child(div().flex_1().child(Input::new(&self.input)))
                    .when(count > 0, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(t!("Panel.count", count = count).to_string()),
                        )
                    }),
            )
            .when(unreadable, |d| {
                d.child(
                    div()
                        .px_4()
                        .py_1()
                        .text_xs()
                        .bg(theme.muted)
                        .text_color(theme.muted_foreground)
                        .child(t!("Panel.menu_unreadable").to_string()),
                )
            })
            .child(self.render_body(cx))
            .child(self.render_footer(cx))
    }
}

#[cfg(test)]
mod tests {
    // `use gpui_kit::*` imports gpui's test macro, shadowing the built-in #[test];
    // the #[test] that #[gpui_kit::test] expands to must resolve to the built-in macro, or it recurses infinitely.
    use core::prelude::v1::test;
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::TestAppContext;
    use keylens_core::{AppShortcuts, EntryId, Group, Shortcut};

    use super::*;

    fn data() -> AppShortcuts {
        let entry = |index: u32, title: &str, keys: &str, enabled: bool| Entry {
            id: EntryId {
                source: EntrySource::Menu,
                snapshot: 1,
                index,
            },
            group: "文件".into(),
            path: vec![],
            title: title.into(),
            search_terms: vec![],
            shortcut: Shortcut::parse_seq(&[keys], Os::MacOS).unwrap(),
            enabled,
        };
        AppShortcuts {
            app: AppInfo {
                pid: 1,
                name: "Demo".into(),
                ids: vec![],
            },
            groups: vec![Group {
                title: "文件".into(),
                community: false,
                entries: vec![
                    entry(0, "新建", "Mod+N", true),
                    entry(1, "关闭", "Mod+W", false),
                    entry(2, "打开", "Mod+O", true),
                ],
            }],
            warnings: vec![],
        }
    }

    fn setup(cx: &mut TestAppContext) -> (WindowHandle<PanelView>, Rc<RefCell<Vec<PanelEvent>>>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let window = cx.add_window(|window, cx| PanelView::new(Os::MacOS, window, cx));
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        window
            .update(cx, |view, window, cx| {
                view.set_data(data(), None, cx);
                view.focus_input(window, cx);
                cx.subscribe_in(
                    &cx.entity(),
                    window,
                    move |_, _, event: &PanelEvent, _, _| {
                        sink.borrow_mut().push(event.clone());
                    },
                )
                .detach();
            })
            .unwrap();
        (window, events)
    }

    fn executed(events: &Rc<RefCell<Vec<PanelEvent>>>) -> Vec<String> {
        events
            .borrow()
            .iter()
            .filter_map(|e| match e {
                PanelEvent::Execute(entry) => Some(entry.title.clone()),
                _ => None,
            })
            .collect()
    }

    #[gpui_kit::test]
    fn enter_executes_the_selected_entry_once(cx: &mut TestAppContext) {
        let (window, events) = setup(cx);
        cx.simulate_keystrokes(window.into(), "down down enter");
        assert_eq!(executed(&events), ["打开"]);
    }

    #[gpui_kit::test]
    fn disabled_entries_are_not_executed(cx: &mut TestAppContext) {
        let (window, events) = setup(cx);
        cx.simulate_keystrokes(window.into(), "down enter");
        assert!(executed(&events).is_empty());
    }

    #[gpui_kit::test]
    fn clicking_a_row_keeps_typing_in_the_search_box(cx: &mut TestAppContext) {
        let (window, _) = setup(cx);
        let focused = window
            .update(cx, |view, window, cx| {
                // A mouse-down on a row moves focus away from the input (gpui focuses the hit element).
                view.focus.focus(window, cx);
                view.click(2, 1, window, cx);
                view.input.read(cx).focus_handle(cx).is_focused(window)
            })
            .unwrap();
        assert!(focused);
    }

    #[gpui_kit::test]
    fn reset_selects_the_first_entry_again(cx: &mut TestAppContext) {
        let (window, _) = setup(cx);
        cx.simulate_keystrokes(window.into(), "down down");
        let title = window
            .update(cx, |view, window, cx| {
                view.reset(window, cx);
                view.model().selected_entry().map(|e| e.title.clone())
            })
            .unwrap();
        assert_eq!(title.as_deref(), Some("新建"));
    }

    #[gpui_kit::test]
    fn selected_row_bounds_include_the_list_padding(cx: &mut TestAppContext) {
        let (window, _) = setup(cx);
        let bounds = window
            .update(cx, |view, _, _| view.selected_bounds())
            .unwrap();
        // Padding, then the "文件" header, then the first row.
        assert_eq!(
            bounds,
            Some((
                LIST_PADDING + HEADER_HEIGHT,
                LIST_PADDING + HEADER_HEIGHT + ROW_HEIGHT
            ))
        );
    }

    #[gpui_kit::test]
    fn escape_clears_search_before_dismissing(cx: &mut TestAppContext) {
        let (window, events) = setup(cx);
        window
            .update(cx, |view, window, cx| {
                view.input
                    .update(cx, |input, cx| input.set_value("打开", window, cx));
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "escape");
        let query = window
            .update(cx, |view, _, cx| view.input.read(cx).value().to_string())
            .unwrap();
        assert_eq!(query, "");
        assert!(!events.borrow().contains(&PanelEvent::Dismiss));
        cx.simulate_keystrokes(window.into(), "escape");
        assert!(events.borrow().contains(&PanelEvent::Dismiss));
    }
}
