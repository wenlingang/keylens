//! Panel view: renders `PanelModel`, handles keys and mouse, and emits events to the controller.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use keylens_core::{AppInfo, AppShortcuts, Entry, EntrySource, KeyQuery, Os, Shortcut};
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
        Dismiss,
        ShowSettings
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

/// How long each keycap of a shortcut takes to go down before it runs, and the pause after the
/// last one, so the user sees what was pressed.
const PRESS_STEP: Duration = Duration::from_millis(55);
const PRESS_TAIL: Duration = Duration::from_millis(130);
/// Columns rise into place one after another when the panel appears.
const APPEAR: Duration = Duration::from_millis(260);
const APPEAR_STAGGER: Duration = Duration::from_millis(45);
const RAINBOW_FOR: Duration = Duration::from_secs(5);
/// ↑↑↓↓←→←→BA inside the panel turns the keycaps into a rainbow for a few seconds.
const KONAMI: [&str; 10] = [
    "up", "up", "down", "down", "left", "right", "left", "right", "b", "a",
];
const SLEEPY_KEYCAP: &[u8] = include_bytes!("../../../assets/sleepy-keycap.svg");
const NO_MATCH_QUIPS: usize = 4;

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some(KEY_CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        KeyBinding::new("left", SelectLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("right", SelectRight, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Execute, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(KEY_CONTEXT)),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-,"
            } else {
                "ctrl-,"
            },
            ShowSettings,
            Some(KEY_CONTEXT),
        ),
    ]);
}

#[derive(Clone, Debug, PartialEq)]
pub enum PanelEvent {
    Execute(Entry),
    Dismiss,
    /// The panel lost keyboard focus. The controller decides whether that dismisses it: not while
    /// focus is in Keylens' own settings window.
    Deactivated,
    /// Open the system settings where Keylens' permission is granted.
    OpenPermissionSettings,
    /// Open Keylens' own settings window.
    OpenSettings,
    /// Create (if missing) and open the user data file for this app.
    EditShortcuts(AppInfo),
}

/// The shown app's icon and the colour taken from it (`None`: a grey icon, use the default).
#[derive(Clone)]
pub struct AppIcon {
    pub image: Arc<Image>,
    pub accent: Option<Hsla>,
}

/// Values every part of one render needs.
struct Paint {
    accent: Hsla,
    dark: bool,
    keys: KeyQuery,
}

pub struct PanelView {
    model: PanelModel,
    input: Entity<InputState>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    icon: Option<AppIcon>,
    viewport: f32,
    /// Bumped each time the panel appears, so its entrance animation plays again.
    appearance: usize,
    /// Whether the panel is on screen. Repeating animations only run while it is.
    shown: bool,
    /// The entry whose keycaps are going down before it runs, with a counter that keeps each
    /// press animation distinct.
    pressing: Option<(usize, usize)>,
    presses: usize,
    rainbow: bool,
    /// The last few keys pressed in the panel, to spot ↑↑↓↓←→←→BA.
    recent_keys: Vec<SharedString>,
    sleepy: Arc<Image>,
    _press: Option<Task<()>>,
    _rainbow: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PanelEvent> for PanelView {}

impl PanelView {
    pub fn new(os: Os, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Panel.search_placeholder").to_string())
        });
        let on_input = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                // Enter only goes through the Execute action; only text changes are handled here.
                if let InputEvent::Change = event {
                    let query = input.read(cx).value().to_string();
                    // Typed characters reach the box through text input, not as key events: a query
                    // that grew by one character is how the panel sees a letter being typed.
                    if let Some(typed) = query.strip_prefix(this.model.query())
                        && typed.chars().count() == 1
                    {
                        this.note_key(&typed.to_lowercase(), window, cx);
                    }
                    this.model.set_query(&query);
                    this.scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }
            },
        );
        let on_activation = cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() {
                cx.emit(PanelEvent::Deactivated);
            }
        });
        let on_appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
        });
        // Named keys (the arrows) are seen here whoever handles them: the panel's selection or
        // the search box's cursor. Typed characters are seen as the query grows (above).
        let panel = window.window_handle();
        let on_keys = cx.observe_keystrokes(move |this, event, window, cx| {
            let key = &event.keystroke.key;
            if window.window_handle() == panel && key.chars().count() > 1 {
                this.note_key(key, window, cx);
            }
        });
        Self {
            model: PanelModel::new(os),
            input,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            icon: None,
            viewport: 400.0,
            appearance: 0,
            shown: false,
            pressing: None,
            presses: 0,
            rainbow: false,
            recent_keys: Vec::new(),
            sleepy: Arc::new(Image::from_bytes(ImageFormat::Svg, SLEEPY_KEYCAP.to_vec())),
            _press: None,
            _rainbow: None,
            _subscriptions: vec![on_input, on_activation, on_appearance, on_keys],
        }
    }

    pub fn model(&self) -> &PanelModel {
        &self.model
    }

    pub fn set_loading(&mut self, app: AppInfo, icon: Option<AppIcon>, cx: &mut Context<Self>) {
        self.icon = icon;
        self.model.set_loading(app);
        cx.notify();
    }

    pub fn set_data(&mut self, data: AppShortcuts, icon: Option<AppIcon>, cx: &mut Context<Self>) {
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

    /// The panel is about to come on screen: replay the entrance animation.
    pub fn appear(&mut self, cx: &mut Context<Self>) {
        self.appearance += 1;
        self.shown = true;
        cx.notify();
    }

    /// The panel left the screen: stop anything still animating.
    pub fn disappear(&mut self, cx: &mut Context<Self>) {
        self.shown = false;
        self.rainbow = false;
        self.pressing = None;
        self._press = None;
        self._rainbow = None;
        self.recent_keys.clear();
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

    fn show_settings(&mut self, _: &ShowSettings, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PanelEvent::OpenSettings);
    }

    /// Re-reads translated strings that were captured when the view was built.
    pub fn refresh_locale(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_placeholder(t!("Panel.search_placeholder").to_string(), window, cx)
        });
        cx.notify();
    }

    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        // Trust the text in the input box: setting text programmatically doesn't always fire a Change event.
        if self.input.read(cx).value().is_empty() {
            cx.emit(PanelEvent::Dismiss);
        } else {
            self.reset(window, cx);
        }
    }

    /// Presses the selected entry's keycaps one by one, then asks the controller to run it.
    fn execute_selected(&mut self, cx: &mut Context<Self>) {
        if self.pressing.is_some() {
            return;
        }
        let Some(index) = self.model.selected_index() else {
            return;
        };
        let entry = self.model.entry(index).clone();
        if !entry.enabled {
            return;
        }
        let caps = entry
            .shortcut
            .chords()
            .iter()
            .map(|c| c.caps(self.model.os()).len())
            .sum::<usize>() as u32;
        self.presses += 1;
        self.pressing = Some((index, self.presses));
        let delay = PRESS_STEP * caps + PRESS_TAIL;
        self._press = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                this.pressing = None;
                cx.emit(PanelEvent::Execute(entry));
                cx.notify();
            });
        }));
        cx.notify();
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

    /// Remembers the last few keys pressed in the panel, to spot ↑↑↓↓←→←→BA.
    fn note_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.recent_keys.push(SharedString::from(key.to_string()));
        if self.recent_keys.len() > KONAMI.len() {
            self.recent_keys.remove(0);
        }
        if self.recent_keys.iter().map(|k| k.as_ref()).eq(KONAMI) {
            self.recent_keys.clear();
            // The B and A went into the search box (the input sees them after this listener).
            cx.defer_in(window, |this, window, cx| this.reset(window, cx));
            self.rainbow = true;
            self._rainbow = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(RAINBOW_FOR).await;
                let _ = this.update(cx, |this, cx| {
                    this.rainbow = false;
                    cx.notify();
                });
            }));
            cx.notify();
        }
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

    fn paint(&self, cx: &App) -> Paint {
        Paint {
            accent: self
                .icon
                .as_ref()
                .and_then(|icon| icon.accent)
                .unwrap_or_else(|| rgb(crate::charge::DEFAULT_ACCENT).into()),
            dark: cx.theme().is_dark(),
            keys: KeyQuery::parse(self.model.query(), self.model.os()),
        }
    }

    /// A shortcut as a row of keycaps. `cap_index` numbers keycaps across the whole panel so the
    /// rainbow ripples from one to the next.
    fn keycaps(
        &self,
        entry: usize,
        shortcut: &Shortcut,
        paint: &Paint,
        cap_index: &mut usize,
        cx: &App,
    ) -> Div {
        let theme = cx.theme();
        let os = self.model.os();
        let press = self
            .pressing
            .filter(|(pressed, _)| *pressed == entry)
            .map(|(_, n)| n);
        let mut caps = h_flex().gap(px(3.)).flex_none();
        let mut nth = 0;
        for (i, chord) in shortcut.chords().iter().enumerate() {
            if i > 0 {
                caps = caps.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("→"),
                );
            }
            let lit = paint.keys.lit_caps(chord, os);
            for ((_, label), lit) in chord.labeled_caps(os).into_iter().zip(lit) {
                let cap = keycap(label, lit, paint);
                let index = *cap_index;
                *cap_index += 1;
                caps = caps.child(if let Some(n) = press {
                    let step = nth as f32;
                    nth += 1;
                    pressed_cap(cap, n * 64 + nth, step)
                } else if self.rainbow && self.shown {
                    rainbow_cap(cap, index).into_any_element()
                } else {
                    cap.into_any_element()
                });
            }
        }
        caps
    }

    fn title(
        text: &str,
        highlight: Option<Range<usize>>,
        path: Option<String>,
        paint: &Paint,
        cx: &App,
    ) -> Div {
        let theme = cx.theme();
        let mut title = h_flex().flex_1().min_w_0().overflow_hidden();
        match highlight {
            Some(range) => {
                title = title
                    .child(text[..range.start].to_string())
                    .child(
                        div()
                            .bg(paint.accent.opacity(0.28))
                            .rounded(px(3.))
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

    fn render_header(
        title: &str,
        community: bool,
        count: usize,
        paint: &Paint,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .h(px(HEADER_HEIGHT))
            .px_2()
            .gap(px(6.))
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .size(px(14.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .bg(paint.accent.opacity(0.14))
                    .child(div().size(px(6.)).rounded(px(3.)).bg(paint.accent)),
            )
            .child(title.to_string())
            .when(community, |d| {
                d.child(div().text_color(theme.link).child("◇"))
            })
            .child(div().flex_1())
            .child(
                div()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(theme.muted_foreground.opacity(0.7))
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    fn render_row(
        &self,
        column: usize,
        row: usize,
        count: usize,
        paint: &Paint,
        cap_index: &mut usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        match &self.model.layout()[column][row] {
            Row::Header { title, community } => {
                Self::render_header(title, *community, count, paint, cx)
            }
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
                    .relative()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .gap_2()
                    .rounded(px(7.))
                    .text_sm()
                    .when(selected, |d| {
                        d.bg(paint.accent.opacity(0.15)).child(
                            // Indicator bar on the left edge of the selected row.
                            div()
                                .absolute()
                                .left(px(2.))
                                .top(px(6.))
                                .bottom(px(6.))
                                .w(px(3.))
                                .rounded(px(2.))
                                .bg(paint.accent),
                        )
                    })
                    .when(!selected, |d| d.hover(|s| s.bg(theme.muted.opacity(0.5))))
                    .when(!entry.enabled, |d| d.opacity(0.4))
                    .child(Self::title(
                        &entry.title,
                        highlight.clone(),
                        path,
                        paint,
                        cx,
                    ))
                    .when(entry.source() != EntrySource::Menu, |d| {
                        d.child(div().text_xs().text_color(theme.link).child("◇"))
                    })
                    .child(self.keycaps(index, &entry.shortcut, paint, cap_index, cx))
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
        sleepy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let mut page = v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_2()
            .p_8();
        if sleepy {
            let art = img(self.sleepy.clone()).w(px(120.)).h(px(96.));
            page = page.child(if self.shown {
                // A slow bob, 20 frames a second is plenty.
                art.with_animation(
                    ("sleepy", self.appearance),
                    Animation::new(Duration::from_millis(2600))
                        .repeat()
                        .with_max_fps(20.),
                    |art, t| {
                        let lift = (t * std::f32::consts::PI).sin() * 5.0;
                        art.relative().top(px(-lift))
                    },
                )
                .into_any_element()
            } else {
                art.into_any_element()
            });
        }
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

    fn render_body(&self, paint: &Paint, cx: &mut Context<Self>) -> AnyElement {
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
                        PanelEvent::OpenPermissionSettings,
                    )),
                    false,
                    cx,
                );
            }
            PanelStatus::Loading if !self.model.has_entries() => {
                return self.render_state(t!("Panel.loading").to_string(), vec![], None, false, cx);
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
                    false,
                    cx,
                );
            }
            // Only apps with a bundle ID can get a user data file.
            let add = self
                .model
                .app()
                .filter(|app| !app.ids.is_empty())
                .map(|app| {
                    (
                        t!("Panel.add_shortcuts").to_string(),
                        PanelEvent::EditShortcuts(app.clone()),
                    )
                });
            let app = self.model.app().map(|a| a.name.clone()).unwrap_or_default();
            return self.render_state(
                t!("Panel.empty_title", app = app).to_string(),
                vec![t!("Panel.empty_body").to_string()],
                add,
                true,
                cx,
            );
        }
        if self.model.item_count() == 0 {
            let query = self.model.query();
            // The same query always gets the same line, so it doesn't change while typing on.
            let quip = query.chars().map(|c| c as usize).sum::<usize>() % NO_MATCH_QUIPS;
            return self.render_state(
                t!("Panel.no_match", query = query).to_string(),
                vec![
                    match quip {
                        0 => t!("Panel.no_match_quip_0"),
                        1 => t!("Panel.no_match_quip_1"),
                        2 => t!("Panel.no_match_quip_2"),
                        _ => t!("Panel.no_match_quip_3"),
                    }
                    .to_string(),
                    t!("Panel.no_match_hint").to_string(),
                ],
                None,
                false,
                cx,
            );
        }
        let mut columns = h_flex()
            .items_start()
            .gap_2()
            .px_2()
            .pt(px(LIST_PADDING))
            .pb(px(LIST_PADDING));
        let mut cap_index = 0;
        for (c, column) in self.model.layout().iter().enumerate() {
            let mut col = v_flex().flex_1().min_w_0();
            for r in 0..column.len() {
                let count = if matches!(column[r], Row::Header { .. }) {
                    column[r + 1..]
                        .iter()
                        .take_while(|row| matches!(row, Row::Item { .. }))
                        .count()
                } else {
                    0
                };
                col = col.child(self.render_row(c, r, count, paint, &mut cap_index, cx));
            }
            columns = columns.child(appear(col, self.appearance, c));
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
            .when_some(
                self.model.app().filter(|app| !app.ids.is_empty()).cloned(),
                |d, app| {
                    d.child(
                        footer_link("edit-shortcuts", t!("Panel.edit_shortcuts"), cx).on_click(
                            cx.listener(move |_, _, _, cx| {
                                cx.emit(PanelEvent::EditShortcuts(app.clone()))
                            }),
                        ),
                    )
                },
            )
            .child(
                footer_link("open-settings", t!("Panel.settings"), cx)
                    .debug_selector(|| "open-settings".into())
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PanelEvent::OpenSettings))),
            )
            .child(format!("↩ {}", t!("Panel.execute")))
            .child(format!("esc {}", t!("Panel.close")))
            .into_any_element()
    }
}

/// A keycap: light face, thicker bottom edge like a real key. Lit caps (matching a key search)
/// take the accent colour.
fn keycap(label: String, lit: bool, paint: &Paint) -> Div {
    let (face, edge, text) = if lit {
        (
            paint.accent,
            Hsla {
                l: paint.accent.l * 0.7,
                ..paint.accent
            },
            gpui_kit::white(),
        )
    } else if paint.dark {
        (
            hsla(0., 0., 0.30, 1.),
            hsla(0., 0., 0., 0.55),
            hsla(0., 0., 0.92, 1.),
        )
    } else {
        (
            hsla(0., 0., 1., 1.),
            hsla(0., 0., 0., 0.18),
            hsla(0., 0., 0.23, 1.),
        )
    };
    div()
        .h(px(20.))
        .min_w(px(20.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .rounded(px(5.))
        .bg(face)
        .border_1()
        .border_b_2()
        .border_color(edge)
        .text_color(text)
        .child(label)
}

/// A keycap that goes down at its turn while the entry is being run; `step` is its position in
/// the shortcut, so the first cap goes down first.
fn pressed_cap(cap: Div, id: usize, step: f32) -> AnyElement {
    let start = PRESS_STEP.as_secs_f32() * step;
    let total = start + PRESS_STEP.as_secs_f32() + PRESS_TAIL.as_secs_f32();
    let down_from = start / total;
    cap.with_animation(
        ("press", id),
        Animation::new(Duration::from_secs_f32(total)),
        move |cap, t| {
            if t >= down_from {
                cap.relative().top(px(1.)).border_b_1()
            } else {
                cap
            }
        },
    )
    .into_any_element()
}

/// A keycap cycling through the hues, a little behind the one before it.
fn rainbow_cap(cap: Div, index: usize) -> impl IntoElement {
    let offset = index as f32 * 0.035;
    cap.with_animation(
        ("rainbow", index),
        Animation::new(Duration::from_millis(1600)).repeat(),
        move |cap, t| {
            let hue = (t + offset).fract();
            let lift = ((t + offset) * std::f32::consts::TAU).sin().max(0.0) * 2.0;
            cap.bg(hsla(hue, 0.8, 0.58, 1.))
                .border_color(hsla(hue, 0.8, 0.40, 1.))
                .text_color(gpui_kit::white())
                .relative()
                .top(px(-lift))
        },
    )
}

/// A column fading and rising into place, a little after the column to its left.
fn appear(column: Div, appearance: usize, index: usize) -> AnyElement {
    let delay = APPEAR_STAGGER.as_secs_f32() * index as f32;
    let total = delay + APPEAR.as_secs_f32();
    let from = delay / total;
    column
        .with_animation(
            ("appear", appearance * 32 + index),
            Animation::new(Duration::from_secs_f32(total)).with_easing(move |t| {
                if t <= from {
                    0.0
                } else {
                    let t = (t - from) / (1.0 - from);
                    1.0 - (1.0 - t).powi(3)
                }
            }),
            |column, v| column.opacity(v).relative().top(px((1.0 - v) * 8.0)),
        )
        .into_any_element()
}

/// A clickable text link in the footer, in the theme's link colour (`accent` is a hover
/// background and nearly invisible as text).
fn footer_link(id: &'static str, label: impl ToString, cx: &Context<PanelView>) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .text_color(theme.link)
        .underline()
        .cursor_pointer()
        .hover(|style| style.text_color(theme.link_hover))
        .child(label.to_string())
}

impl Render for PanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = window.bounds();
        let width = f32::from(bounds.size.width);
        self.viewport = (f32::from(bounds.size.height) - CHROME_HEIGHT).max(ROW_HEIGHT);
        self.model
            .set_columns(((width - 16.0) / COLUMN_WIDTH).floor().max(1.0) as usize);

        let paint = self.paint(cx);
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
            .on_action(cx.listener(Self::show_settings))
            .relative()
            .size_full()
            // Translucent over the window's blurred background.
            .bg(theme.popover.opacity(0.82))
            .text_color(theme.foreground)
            .rounded(px(14.))
            .border_1()
            .border_color(theme.border)
            .child(
                // A thin strip of the app's colour along the top edge.
                div()
                    .absolute()
                    .top_0()
                    .left(px(24.))
                    .right(px(24.))
                    .h(px(3.))
                    .rounded_b(px(3.))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(paint.accent, 0.),
                        linear_color_stop(paint.accent.opacity(0.25), 1.),
                    )),
            )
            .child(
                h_flex()
                    .h(px(56.))
                    .px_4()
                    .gap_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .when_some(self.icon.as_ref().map(|i| i.image.clone()), |d, icon| {
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
            .child(self.render_body(&paint, cx))
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
    fn cmd_comma_asks_for_settings(cx: &mut TestAppContext) {
        let (window, events) = setup(cx);
        cx.simulate_keystrokes(
            window.into(),
            if cfg!(target_os = "macos") {
                "cmd-,"
            } else {
                "ctrl-,"
            },
        );
        assert!(events.borrow().contains(&PanelEvent::OpenSettings));
    }

    /// Lets the keycap press animation finish, after which the entry runs.
    fn finish_press(cx: &mut TestAppContext) {
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn enter_executes_the_selected_entry_once(cx: &mut TestAppContext) {
        let (window, events) = setup(cx);
        cx.simulate_keystrokes(window.into(), "down down enter");
        // The keycaps go down first; the entry runs after.
        assert!(executed(&events).is_empty());
        finish_press(cx);
        assert_eq!(executed(&events), ["打开"]);
    }

    #[gpui_kit::test]
    fn enter_again_while_pressing_does_not_run_twice(cx: &mut TestAppContext) {
        let (window, events) = setup(cx);
        cx.simulate_keystrokes(window.into(), "enter enter");
        finish_press(cx);
        assert_eq!(executed(&events), ["新建"]);
    }

    #[gpui_kit::test]
    fn konami_code_turns_on_the_rainbow_and_clears_the_search(cx: &mut TestAppContext) {
        let (window, _) = setup(cx);
        cx.simulate_keystrokes(window.into(), "up up down down left right left right b a");
        cx.run_until_parked();
        let (rainbow, query) = window
            .update(cx, |view, _, cx| {
                (view.rainbow, view.input.read(cx).value().to_string())
            })
            .unwrap();
        assert!(rainbow);
        assert_eq!(query, "");
        cx.executor().advance_clock(RAINBOW_FOR);
        cx.run_until_parked();
        assert!(!window.update(cx, |view, _, _| view.rainbow).unwrap());
    }

    #[gpui_kit::test]
    fn disabled_entries_are_not_executed(cx: &mut TestAppContext) {
        let (window, events) = setup(cx);
        cx.simulate_keystrokes(window.into(), "down enter");
        finish_press(cx);
        assert!(executed(&events).is_empty());
    }

    #[gpui_kit::test]
    fn footer_settings_link_asks_for_settings(cx: &mut TestAppContext) {
        let (window, events) = setup(cx);
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        let bounds = cx
            .debug_bounds("open-settings")
            .expect("the footer shows a settings link");
        cx.simulate_click(bounds.center(), Modifiers::none());
        assert!(events.borrow().contains(&PanelEvent::OpenSettings));
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
