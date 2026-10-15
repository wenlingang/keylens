//! The inline add/edit form of the shortcut editor, with its key recorder.

use std::time::Instant;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use keylens_core::Shortcut;
use rust_i18n::t;

use super::ShortcutEditor;
use crate::recorder::{Pressed, SEQUENCE_WINDOW, chord_text};
use crate::ui::CONTROL_WIDTH;
use crate::user_data_edit::{self, At};

pub(super) struct Form {
    /// The entry being edited; `None` while adding.
    pub(super) target: Option<At>,
    /// The title the entry had when the form opened, checked against the file on update.
    pub(super) original_title: String,
    pub(super) title: Entity<InputState>,
    /// Recorded chords (one, or two for a sequence).
    pub(super) keys: Vec<String>,
    /// `keys` when recording started, put back by Esc.
    pub(super) keys_before: Vec<String>,
    /// The entry's keys differ per platform: they are left as they are.
    pub(super) per_platform: bool,
    pub(super) recording: bool,
    pub(super) last_chord_at: Option<Instant>,
    /// `Some` once the user chose to type the keys instead of recording them.
    pub(super) manual: Option<Entity<InputState>>,
    /// The chosen existing group.
    pub(super) group: String,
    /// `Some` when "New Group…" is chosen.
    pub(super) new_group: Option<Entity<InputState>>,
    pub(super) error: Option<SharedString>,
}

impl ShortcutEditor {
    /// Opens an empty form at the end of the list.
    pub(super) fn start_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only() || self.form.is_some() {
            return;
        }
        let group = self
            .groups
            .first()
            .map(|g| g.title.clone())
            .unwrap_or_else(|| t!("Editor.default_group").to_string());
        self.open_form(None, String::new(), Some(Vec::new()), group, window, cx);
    }

    /// Opens the form in place of the entry at `at`, replacing any open form.
    pub(super) fn start_edit(&mut self, at: At, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only() {
            return;
        }
        let Some(group) = self.groups.get(at.group) else {
            return;
        };
        let Some(shortcut) = group.shortcuts.get(at.index) else {
            return;
        };
        let (title, keys, group) = (
            shortcut.title.clone(),
            shortcut.keys.clone(),
            group.title.clone(),
        );
        self.open_form(Some(at), title, keys, group, window, cx);
    }

    fn open_form(
        &mut self,
        target: Option<At>,
        title: String,
        keys: Option<Vec<String>>,
        group: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(title.clone()));
        input.update(cx, |input, cx| input.focus(window, cx));
        self.form = Some(Form {
            target,
            original_title: title,
            title: input,
            per_platform: keys.is_none(),
            keys: keys.unwrap_or_default(),
            keys_before: Vec::new(),
            recording: false,
            last_chord_at: None,
            manual: None,
            group,
            new_group: None,
            error: None,
        });
        cx.notify();
    }

    pub(super) fn cancel_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.form.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub(super) fn start_recording(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.form.as_mut().filter(|f| !f.per_platform) {
            form.recording = true;
            form.keys_before = form.keys.clone();
            form.last_chord_at = None;
            cx.notify();
        }
    }

    pub(super) fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.form.as_mut() {
            form.recording = false;
            cx.notify();
        }
    }

    /// Records one key press into the form while recording. Returns whether it was used.
    pub(super) fn record_key(
        &mut self,
        keystroke: &Keystroke,
        is_held: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let os = self.os;
        let Some(form) = self.form.as_mut().filter(|f| f.recording) else {
            return false;
        };
        let bare = !keystroke.modifiers.modified();
        if bare && keystroke.key == "escape" {
            form.keys = form.keys_before.clone();
            form.recording = false;
            window.focus(&self.focus, cx);
        } else if bare && (keystroke.key == "backspace" || keystroke.key == "delete") {
            form.keys.clear();
            form.last_chord_at = None;
        } else if is_held {
            // Holding a key repeats it; that is not a second chord.
            return true;
        } else {
            let m = keystroke.modifiers;
            let pressed = Pressed {
                control: m.control,
                alt: m.alt,
                shift: m.shift,
                platform: m.platform,
                function: m.function,
            };
            let Some(chord) = chord_text(pressed, &keystroke.key, os) else {
                return false;
            };
            // The executor's clock, so tests can let time pass.
            let now = cx.background_executor().now();
            let in_sequence = form
                .last_chord_at
                .is_some_and(|at| now.duration_since(at) <= SEQUENCE_WINDOW);
            if in_sequence && form.keys.len() == 1 {
                form.keys.push(chord);
            } else {
                form.keys = vec![chord];
            }
            form.last_chord_at = Some(now);
        }
        form.error = None;
        cx.notify();
        true
    }

    fn type_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut().filter(|f| f.manual.is_none()) else {
            return;
        };
        let text = form.keys.join(" ");
        let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
        input.update(cx, |input, cx| input.focus(window, cx));
        form.manual = Some(input);
        cx.notify();
    }

    /// `None` picks "New Group…".
    pub(super) fn choose_group(
        &mut self,
        group: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        match group {
            Some(group) => {
                form.group = group;
                form.new_group = None;
            }
            None if form.new_group.is_none() => {
                let input = cx.new(|cx| InputState::new(window, cx));
                input.update(cx, |input, cx| input.focus(window, cx));
                form.new_group = Some(input);
            }
            None => {}
        }
        cx.notify();
    }

    /// The title of another entry with the same keys, if any.
    fn duplicate_of(&self, keys: &[String], except: Option<At>) -> Option<String> {
        let mine = Shortcut::parse_seq(keys, self.os).ok()?;
        self.groups.iter().enumerate().find_map(|(group, g)| {
            g.shortcuts.iter().enumerate().find_map(|(index, s)| {
                let theirs = s.keys.as_ref()?;
                (except != Some(At { group, index })
                    && Shortcut::parse_seq(theirs, self.os).ok().as_ref() == Some(&mine))
                .then(|| s.title.clone())
            })
        })
    }

    /// Checks the form against the file as it is now and writes it.
    pub(super) fn save_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Positions and the duplicate check go by the file as it is now; this may close a form
        // whose entry is gone.
        self.reload();
        cx.notify();
        let Some(form) = self.form.as_ref() else {
            return;
        };
        let title = form.title.read(cx).value().trim().to_string();
        let keys = if form.per_platform {
            None
        } else {
            Some(match &form.manual {
                Some(input) => input
                    .read(cx)
                    .value()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                None => form.keys.clone(),
            })
        };
        let group = match &form.new_group {
            Some(input) => input.read(cx).value().trim().to_string(),
            None => form.group.clone(),
        };
        let target = form.target;
        let original = form.original_title.clone();
        // The data format allows each key combination once per app.
        let error = if title.is_empty() {
            Some(t!("Editor.title_required").to_string())
        } else if keys
            .as_ref()
            .is_some_and(|k| k.is_empty() || Shortcut::parse_seq(k, self.os).is_err())
        {
            Some(t!("Editor.invalid_keys").to_string())
        } else if group.is_empty() {
            Some(t!("Editor.group_required").to_string())
        } else {
            keys.as_deref()
                .and_then(|k| self.duplicate_of(k, target))
                .map(|other| t!("Editor.duplicate", title = other).to_string())
        };
        if let Some(error) = error {
            if let Some(form) = &mut self.form {
                form.error = Some(error.into());
            }
            return;
        }
        match target {
            None => self.apply_form(
                |text| user_data_edit::add(text, &group, &title, keys.as_deref().unwrap_or(&[])),
                window,
                cx,
            ),
            Some(at) => self.apply_form(
                |text| user_data_edit::update(text, at, &original, &title, keys.as_deref(), &group),
                window,
                cx,
            ),
        }
    }

    pub(super) fn render_form(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.form.as_ref() else {
            return div().into_any_element();
        };
        let keys_control = match &form.manual {
            Some(input) => Input::new(input).small().into_any_element(),
            None => self.render_recorder(form, window, cx),
        };
        let (group_control, group_menu) = match &form.new_group {
            Some(input) => (
                Input::new(input).small().into_any_element(),
                Some(self.group_dropdown(form, cx).into_any_element()),
            ),
            None => (self.group_dropdown(form, cx).into_any_element(), None),
        };
        let theme = cx.theme();
        let manual_link = (form.manual.is_none() && !form.per_platform).then(|| {
            div()
                .id("type-keys")
                .text_xs()
                .text_color(theme.link)
                .cursor_pointer()
                .hover(|style| style.text_color(theme.link_hover))
                .child(t!("Editor.manual").to_string())
                .on_click(cx.listener(|this, _, window, cx| this.type_keys(window, cx)))
        });
        v_flex()
            .id("form")
            .w_full()
            .py_2()
            .gap_1()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "enter" => this.save_form(window, cx),
                    "escape" => this.cancel_form(window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(form_row(
                t!("Editor.title_label").to_string(),
                None,
                Input::new(&form.title).small().into_any_element(),
            ))
            .child(form_row(
                t!("Editor.keys_label").to_string(),
                manual_link.map(IntoElement::into_any_element),
                keys_control,
            ))
            .child(form_row(
                t!("Editor.group_label").to_string(),
                group_menu,
                group_control,
            ))
            .when_some(form.error.clone(), |this, error| {
                this.child(div().px_3().text_xs().text_color(theme.danger).child(error))
            })
            .child(
                h_flex()
                    .w_full()
                    .px_3()
                    .pt_1()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel")
                            .small()
                            .label(t!("Editor.cancel"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.cancel_form(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("save")
                            .small()
                            .primary()
                            .label(t!("Editor.save"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.save_form(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// A focusable box that turns key presses into chords while it has focus.
    fn render_recorder(&self, form: &Form, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let focused = self.recorder.is_focused(window);
        let shown = |keys: &[String]| {
            Shortcut::parse_seq(keys, self.os)
                .map(|s| s.display(self.os))
                .unwrap_or_else(|_| keys.join(" "))
        };
        let (text, muted) = if form.per_platform {
            (t!("Editor.per_platform").to_string(), true)
        } else if form.recording && form.last_chord_at.is_none() {
            (t!("Editor.recording").to_string(), true)
        } else if form.keys.is_empty() {
            (t!("Editor.click_to_record").to_string(), true)
        } else {
            (shown(&form.keys), false)
        };
        div()
            .id("recorder")
            .w_full()
            .h(px(24.))
            .px_2()
            .flex()
            .items_center()
            .rounded(theme.radius)
            .border_1()
            .border_color(if focused { theme.ring } else { theme.input })
            .bg(theme.background)
            .text_color(if muted {
                theme.muted_foreground
            } else {
                theme.foreground
            })
            .when(form.per_platform, |this| this.text_xs().opacity(0.6))
            .when(!form.per_platform, |this| {
                this.cursor_pointer()
                    .track_focus(&self.recorder)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if this.record_key(&event.keystroke, event.is_held, window, cx) {
                            cx.stop_propagation();
                        }
                    }))
            })
            .child(div().truncate().child(text))
            .into_any_element()
    }

    /// The group picker: existing groups (or the default one) and "New Group…".
    fn group_dropdown(&self, form: &Form, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut groups: Vec<String> = self.groups.iter().map(|g| g.title.clone()).collect();
        if !groups.contains(&form.group) {
            groups.insert(0, form.group.clone());
        }
        let creating = form.new_group.is_some();
        let current = form.group.clone();
        let label: SharedString = if creating {
            t!("Editor.new_group").to_string().into()
        } else {
            current.clone().into()
        };
        let editor = cx.entity().downgrade();
        Button::new("group-select")
            .w_full()
            .small()
            .label(label)
            .dropdown_caret(true)
            .outline()
            .when(creating, |this| this.w(px(28.)).label(""))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let options = groups
                    .iter()
                    .map(|g| (Some(g.clone()), SharedString::from(g.clone())))
                    .chain([(None, t!("Editor.new_group").to_string().into())]);
                options.fold(menu, |menu, (value, label)| {
                    let checked = match &value {
                        Some(g) => !creating && *g == current,
                        None => creating,
                    };
                    let editor = editor.clone();
                    menu.item(PopupMenuItem::new(label).checked(checked).on_click(
                        move |_, window, cx| {
                            let value = value.clone();
                            let _ = editor.update(cx, |e, cx| e.choose_group(value, window, cx));
                        },
                    ))
                })
            })
    }
}

/// A form line: the label, an optional accessory just before the control, then the control.
fn form_row(label: String, accessory: Option<AnyElement>, control: AnyElement) -> AnyElement {
    h_flex()
        .w_full()
        .min_h(px(36.))
        .px_3()
        .gap_2()
        .child(div().flex_1().min_w_0().child(label))
        .children(accessory)
        .child(div().w(px(CONTROL_WIDTH)).child(control))
        .into_any_element()
}
