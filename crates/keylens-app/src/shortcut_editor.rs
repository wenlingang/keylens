//! Shortcut editor window: lists, adds, edits and deletes the current app's own shortcuts, kept in
//! its user data file (spec §2). Every change is written at once through `user_data_edit`, which
//! edits the file in place.

mod form;

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable as _, Theme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use keylens_core::community::parse_app_data;
use keylens_core::{AppInfo, Os};
use rust_i18n::t;

use crate::ui::{ChildDeactivated, section};
use crate::user_data_edit::{self, At, GroupView, Removed};
use crate::{config, config_write, template};
use form::Form;

actions!(keylens_shortcut_editor, [CloseEditor]);

const KEY_CONTEXT: &str = "KeylensShortcutEditor";

/// How long the "Deleted … Undo" bar stays.
const UNDO_FOR: Duration = Duration::from_secs(5);

/// The open editor window, so a second request reuses it instead of opening another.
struct EditorWindow(Option<(AnyWindowHandle, WeakEntity<ShortcutEditor>)>);
impl Global for EditorWindow {}

/// Opens the shortcut editor for `app`, or points the open one at it and brings it to the front.
/// Returns the window and its view when a new window was opened; `None` also when `app` has no
/// usable ID (the caller checks that first, see `Keylens::open_editor`).
pub fn open(
    app: AppInfo,
    os: Os,
    open_path: fn(&Path),
    beep: fn(),
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<ShortcutEditor>)> {
    open_in(config::user_data_dir(), app, os, open_path, beep, cx)
}

fn open_in(
    dir: PathBuf,
    app: AppInfo,
    os: Os,
    open_path: fn(&Path),
    beep: fn(),
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<ShortcutEditor>)> {
    // Keylens has no Dock icon; without activating, the window cannot take keyboard focus.
    cx.activate(true);
    if let Some((handle, view)) = cx.try_global::<EditorWindow>().and_then(|w| w.0.clone())
        && let Some(view) = view.upgrade()
        && handle
            .update(cx, |_, window, cx| {
                view.update(cx, |v, cx| v.set_app(app.clone(), window, cx));
                window.activate_window();
            })
            .is_ok()
    {
        return None;
    }
    let path = template::user_file_for(&dir, &app, os)?;
    // Bind once: the global exists from the first opening on.
    if !cx.has_global::<EditorWindow>() {
        cx.bind_keys([KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-w"
            },
            CloseEditor,
            Some(KEY_CONTEXT),
        )]);
    }
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(t!("Editor.title", app = app.name).to_string().into()),
            ..Default::default()
        }),
        kind: WindowKind::Normal,
        focus: true,
        show: true,
        is_resizable: true,
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(560.), px(480.)),
            cx,
        ))),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| ShortcutEditor::new(app, path, os, open_path, beep, window, cx))
    }) {
        Ok((handle, view)) => {
            // `focus: true` is not enough for an Accessory app's window to become key everywhere.
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            cx.set_global(EditorWindow(Some((handle, view.downgrade()))));
            Some((handle, view))
        }
        Err(error) => {
            tracing::warn!(%error, "cannot open the shortcut editor");
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditorProblem {
    /// The file does not parse (or cannot be read): the window is read-only until it is fixed.
    Invalid(String),
    /// The last write failed; the form stays open so the edit can be retried.
    WriteFailed(String),
    /// Deleting or undoing failed.
    Failed(String),
    /// The entry being edited is no longer in the file; its form was closed.
    Stale,
}

pub struct ShortcutEditor {
    app: AppInfo,
    os: Os,
    open_path: fn(&Path),
    beep: fn(),
    /// The app's user data file: the existing one that matches it, else `<user data dir>/<template
    /// file name>` (see `template::user_file_for`).
    path: PathBuf,
    /// The text last read or written ("" when the file does not exist).
    text: String,
    groups: Vec<GroupView>,
    problem: Option<EditorProblem>,
    form: Option<Form>,
    /// The last deletion, while it can be undone; the task hides the undo bar.
    undo: Option<(Removed, Task<()>)>,
    focus: FocusHandle,
    /// The key recorder in the form (one form at a time, so one handle).
    recorder: FocusHandle,
    /// The window title last set, so a language switch updates it on the next frame.
    shown_title: String,
    _subscriptions: Vec<Subscription>,
}

/// Why an edit was not written.
enum WriteError {
    /// The file does not parse; nothing is edited.
    ReadOnly,
    /// The edit itself was refused (e.g. the entry is no longer where it was).
    Edit(String),
    /// Writing the file failed.
    Io(String),
}

impl ShortcutEditor {
    pub fn new(
        app: AppInfo,
        path: PathBuf,
        os: Os,
        open_path: fn(&Path),
        beep: fn(),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let recorder = cx.focus_handle();
        let editor = cx.entity().downgrade();
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    // The file may have been edited by hand meanwhile.
                    this.reload();
                    cx.notify();
                } else {
                    cx.emit(ChildDeactivated);
                }
            }),
            cx.on_focus(&recorder, window, |this, _, cx| this.start_recording(cx)),
            cx.on_blur(&recorder, window, |this, _, cx| this.stop_recording(cx)),
            // Key bindings (⌘W and the like) run before key handlers; while recording, a
            // keystroke that has a binding is taken here, so it is recorded instead.
            cx.intercept_keystrokes(move |event, window, cx| {
                let _ = editor.update(cx, |this, cx| {
                    // Cheap checks first: other windows' keys never reach the keymap scan.
                    if !this.recorder.is_focused(window)
                        || !this.form.as_ref().is_some_and(|f| f.recording)
                    {
                        return;
                    }
                    let (bindings, pending) = cx.key_bindings().borrow().bindings_for_input(
                        std::slice::from_ref(&event.keystroke),
                        &event.context_stack,
                    );
                    if (!bindings.is_empty() || pending)
                        && this.record_key(&event.keystroke, false, window, cx)
                    {
                        cx.stop_propagation();
                    }
                });
            }),
        ];
        window.focus(&focus, cx);
        let mut editor = Self {
            app,
            os,
            open_path,
            beep,
            path,
            text: String::new(),
            groups: Vec::new(),
            problem: None,
            form: None,
            undo: None,
            focus,
            recorder,
            shown_title: String::new(),
            _subscriptions: subscriptions,
        };
        editor.reload();
        editor
    }

    /// Points the window at another app; an open form and the undo bar are dropped. Asking for the
    /// file already shown changes nothing.
    pub fn set_app(&mut self, app: AppInfo, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        let Some(path) = template::user_file_for(dir, &app, self.os) else {
            return;
        };
        if path == self.path {
            return;
        }
        self.path = path;
        self.app = app;
        self.form = None;
        self.undo = None;
        self.groups.clear();
        self.problem = None;
        window.focus(&self.focus, cx);
        self.reload();
        cx.notify();
    }

    fn read_only(&self) -> bool {
        matches!(self.problem, Some(EditorProblem::Invalid(_)))
    }

    /// Reads the file again. A file that does not parse makes the window read-only and keeps the
    /// last good list on screen.
    fn reload(&mut self) {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            // No file there; a regular file where the data folder should be counts the same,
            // and the write then reports the real problem.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                String::new()
            }
            Err(e) => {
                self.problem = Some(EditorProblem::Invalid(e.to_string()));
                return;
            }
        };
        self.text = text;
        let listed = if self.text.is_empty() {
            Ok(Vec::new())
        } else {
            user_data_edit::list(&self.text, self.os)
        };
        match listed {
            Ok(groups) => {
                self.groups = groups;
                if self.read_only() {
                    self.problem = None;
                }
                self.relocate_form();
            }
            Err(error) => self.problem = Some(EditorProblem::Invalid(error)),
        }
    }

    /// Keeps an open edit form on its entry after the list changed: follows the entry when its
    /// title is still found exactly once, otherwise closes the form and says why.
    fn relocate_form(&mut self) {
        let Some((at, title)) = self
            .form
            .as_ref()
            .and_then(|f| Some((f.target?, f.original_title.clone())))
        else {
            return;
        };
        if self.shortcut_title(at).as_deref() == Some(title.as_str()) {
            return;
        }
        let mut found = self.groups.iter().enumerate().flat_map(|(group, g)| {
            g.shortcuts
                .iter()
                .enumerate()
                .filter(|(_, s)| s.title == title)
                .map(move |(index, _)| At { group, index })
        });
        match (found.next(), found.next()) {
            (Some(moved), None) => {
                if let Some(form) = &mut self.form {
                    form.target = Some(moved);
                }
            }
            _ => {
                self.form = None;
                self.problem = Some(EditorProblem::Stale);
            }
        }
    }

    /// The text to edit: the file, or the template when the file is missing or holds nothing
    /// but comments (comments are kept above the template).
    fn base_text(&self) -> String {
        let blank = self
            .text
            .parse::<toml_edit::DocumentMut>()
            .is_ok_and(|doc| doc.as_table().is_empty());
        if !blank {
            return self.text.clone();
        }
        let template = template::user_data_template(&self.app)
            .map(|(_, text)| text)
            .unwrap_or_default();
        let kept = self.text.trim();
        if kept.is_empty() {
            template
        } else {
            format!("{kept}\n\n{template}")
        }
    }

    /// Applies `edit` to the file as it is on disk now and writes the result.
    fn write(
        &mut self,
        edit: impl FnOnce(&str) -> Result<String, String>,
        cx: &mut Context<Self>,
    ) -> Result<(), WriteError> {
        self.reload();
        cx.notify();
        if self.read_only() {
            return Err(WriteError::ReadOnly);
        }
        let text = edit(&self.base_text()).map_err(WriteError::Edit)?;
        // The safety net behind the form's checks: never write a file the panel cannot load
        // (e.g. keys taken on another platform the file matches).
        let file = self.path.file_name().unwrap_or_default().to_string_lossy();
        parse_app_data(&file, &text).map_err(|e| WriteError::Edit(e.message))?;
        config_write::write_atomic(&self.path, &text).map_err(|e| WriteError::Io(e.to_string()))?;
        self.groups = user_data_edit::list(&text, self.os).unwrap_or_default();
        self.text = text;
        self.problem = None;
        Ok(())
    }

    /// Writes the open form's edit. On success the form closes; otherwise it stays, with the
    /// reason in the form or (for a failed write) in the banner.
    fn apply_form(
        &mut self,
        edit: impl FnOnce(&str) -> Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.write(edit, cx) {
            Ok(()) => {
                self.form = None;
                window.focus(&self.focus, cx);
            }
            Err(WriteError::ReadOnly) => {}
            Err(WriteError::Edit(error)) => {
                if let Some(form) = &mut self.form {
                    form.error = Some(error.into());
                }
            }
            Err(WriteError::Io(error)) => {
                self.problem = Some(EditorProblem::WriteFailed(error));
            }
        }
    }

    /// Writes an edit made outside the form (delete, undo); an open form stays as it is.
    fn apply_outside_form(
        &mut self,
        edit: impl FnOnce(&str) -> Result<String, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.write(edit, cx) {
            Ok(()) => {
                self.relocate_form();
                true
            }
            Err(WriteError::ReadOnly) => false,
            Err(WriteError::Edit(error) | WriteError::Io(error)) => {
                self.problem = Some(EditorProblem::Failed(error));
                false
            }
        }
    }

    fn delete_at(&mut self, at: At, _: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.shortcut_title(at) else {
            return;
        };
        let mut removed = None;
        let deleted = self.apply_outside_form(
            |text| {
                let (text, r) = user_data_edit::remove(text, at, &title)?;
                removed = Some(r);
                Ok(text)
            },
            cx,
        );
        if let (true, Some(removed)) = (deleted, removed) {
            let hide = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(UNDO_FOR).await;
                let _ = this.update(cx, |this, cx| {
                    this.undo = None;
                    cx.notify();
                });
            });
            self.undo = Some((removed, hide));
        }
    }

    /// Puts the last deleted entry back. On failure the undo bar stays, so it can be retried.
    fn undo(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some((removed, _)) = &self.undo else {
            return;
        };
        let removed = removed.clone();
        if self.apply_outside_form(|text| user_data_edit::restore(text, &removed), cx) {
            self.undo = None;
        }
    }

    fn shortcut_title(&self, at: At) -> Option<String> {
        self.groups
            .get(at.group)?
            .shortcuts
            .get(at.index)
            .map(|s| s.title.clone())
    }

    /// Opens the file in the default editor, creating it from the template if it does not exist.
    fn open_in_editor(&self) {
        if self.path.exists() {
            (self.open_path)(&self.path);
            return;
        }
        let dir = self.path.parent().unwrap_or(Path::new("."));
        match template::create_user_file(dir, &self.app) {
            Ok(Some(path)) => (self.open_path)(&path),
            Ok(None) => (self.beep)(),
            Err(error) => {
                tracing::warn!(%error, "cannot create the user data file");
                (self.beep)();
            }
        }
    }

    fn render_problem(&self, problem: &EditorProblem, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, can_open) = match problem {
            EditorProblem::Invalid(error) => {
                (t!("Editor.invalid", error = error).to_string(), true)
            }
            EditorProblem::WriteFailed(error) => {
                (t!("Editor.write_failed", error = error).to_string(), false)
            }
            EditorProblem::Failed(error) => {
                (t!("Editor.change_failed", error = error).to_string(), false)
            }
            EditorProblem::Stale => (t!("Editor.stale").to_string(), false),
        };
        h_flex()
            .w_full()
            .p_2()
            .gap_2()
            .rounded_md()
            .bg(cx.theme().danger.opacity(0.12))
            .text_color(cx.theme().danger)
            .child(div().flex_1().child(text))
            .when(can_open, |this| {
                this.child(
                    Button::new("open-file")
                        .small()
                        .label(t!("Editor.open_in_editor"))
                        .on_click(cx.listener(|this, _, _, _| this.open_in_editor())),
                )
            })
    }

    fn render_row(&self, at: At, cx: &mut Context<Self>) -> AnyElement {
        let shortcut = &self.groups[at.group].shortcuts[at.index];
        let read_only = self.read_only();
        let theme = cx.theme();
        let id = format!("{}-{}", at.group, at.index);
        h_flex()
            .w_full()
            .min_h(px(40.))
            .px_3()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(shortcut.title.clone()),
            )
            .child(
                div()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(theme.muted_foreground)
                    .child(shortcut.display.clone()),
            )
            .child(
                Button::new(SharedString::from(format!("edit-{id}")))
                    .ghost()
                    .xsmall()
                    .label("✎")
                    .tooltip(t!("Editor.edit"))
                    .disabled(read_only)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.start_edit(at, window, cx)),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("delete-{id}")))
                    .ghost()
                    .xsmall()
                    .label("🗑")
                    .tooltip(t!("Editor.delete"))
                    .disabled(read_only)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.delete_at(at, window, cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_list(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let editing = self.form.as_ref().and_then(|f| f.target);
        let mut sections: Vec<AnyElement> = Vec::new();
        for (g, group) in self.groups.iter().enumerate() {
            let rows = (0..group.shortcuts.len())
                .map(|index| {
                    let at = At { group: g, index };
                    if editing == Some(at) {
                        self.render_form(window, cx)
                    } else {
                        self.render_row(at, cx)
                    }
                })
                .collect();
            sections.push(section(group.title.clone(), rows, None, cx).into_any_element());
        }
        if self.groups.iter().all(|g| g.shortcuts.is_empty()) && self.form.is_none() {
            sections.push(
                div()
                    .px_3()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("Editor.empty").to_string())
                    .into_any_element(),
            );
        }
        if self.form.as_ref().is_some_and(|f| f.target.is_none()) {
            let form = self.render_form(window, cx);
            sections.push(
                section(t!("Editor.add").to_string(), vec![form], None, cx).into_any_element(),
            );
        }
        sections
    }

    fn render_undo(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let (removed, _) = self.undo.as_ref()?;
        Some(
            h_flex()
                .w_full()
                .px_3()
                .py_1()
                .gap_2()
                .rounded_md()
                .bg(cx.theme().foreground.opacity(0.08))
                .child(
                    div()
                        .flex_1()
                        .child(t!("Editor.deleted", title = removed.title).to_string()),
                )
                .child(
                    Button::new("undo")
                        .ghost()
                        .small()
                        .label(t!("Editor.undo"))
                        .on_click(cx.listener(|this, _, window, cx| this.undo(window, cx))),
                ),
        )
    }
}

impl EventEmitter<ChildDeactivated> for ShortcutEditor {}

impl Render for ShortcutEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Set here so it follows a UI language switch.
        let title = t!("Editor.title", app = self.app.name).to_string();
        if title != self.shown_title {
            window.set_window_title(&title);
            self.shown_title = title;
        }
        // A form closed by a reload may have held focus; keep keys (⌘W, …) working.
        if self.form.is_none() && (window.focused(cx).is_none() || self.recorder.is_focused(window))
        {
            window.focus(&self.focus, cx);
        }
        let read_only = self.read_only();
        let problem = self.problem.clone();
        let list = self.render_list(window, cx);
        let undo = self.render_undo(cx);
        let theme = cx.theme();
        let link = theme.link;
        let link_hover = theme.link_hover;

        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(|_: &CloseEditor, window, _| window.remove_window())
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(
                v_flex()
                    .id("editor-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_4()
                    .pt_3()
                    .pb_4()
                    .gap_6()
                    .when_some(problem, |this, problem| {
                        this.child(self.render_problem(&problem, cx))
                    })
                    .children(list),
            )
            .child(
                v_flex()
                    .w_full()
                    .px_4()
                    .pb_3()
                    .gap_2()
                    .children(undo)
                    .child(
                        // The ghost button pads its label by 8 px; 4 px more lines it up with
                        // the section titles.
                        h_flex()
                            .w_full()
                            .px_1()
                            .justify_between()
                            .child(
                                Button::new("add")
                                    .ghost()
                                    .small()
                                    .label(format!("＋ {}", t!("Editor.add")))
                                    .disabled(read_only || self.form.is_some())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.start_add(window, cx)
                                    })),
                            )
                            .child(
                                div()
                                    .id("open-in-editor")
                                    .text_color(link)
                                    .underline()
                                    .cursor_pointer()
                                    .hover(move |style| style.text_color(link_hover))
                                    .child(format!("{} ↗", t!("Editor.open_in_editor")))
                                    .on_click(cx.listener(|this, _, _, _| this.open_in_editor())),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
