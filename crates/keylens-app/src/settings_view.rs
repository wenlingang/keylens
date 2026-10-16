//! Settings window: edits `Prefs` (the trigger and the UI language). Every change applies at once.

use std::ops::RangeInclusive;
use std::path::Path;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState, NumberInput};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable as _, Sizable as _, Theme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use keylens_core::{Modifiers, Os};
use rust_i18n::t;

use crate::config::{DOUBLE_TAP_MS, DOUBLE_TAP_STEP, HOLD_MS, HOLD_STEP, TriggerMode};
use crate::config_write::Change;
use crate::prefs::{Prefs, Problem};
use crate::ui::{CONTROL_WIDTH, ChildDeactivated, section};

actions!(keylens_settings, [CloseSettings]);

const KEY_CONTEXT: &str = "KeylensSettings";

/// The restore button's slot; reserved even while empty so controls never shift.
const RESET_SLOT: f32 = 24.;

/// UI languages offered besides "System default", as (config value, native name).
const LANGUAGES: [(&str, &str); 5] = [
    ("zh-CN", "简体中文"),
    ("en", "English"),
    ("zh-TW", "繁體中文"),
    ("zh-HK", "繁體中文（香港）"),
    ("ja", "日本語"),
];

/// The open settings window, so a second request focuses it instead of opening another.
#[derive(Default)]
struct SettingsWindow(Option<AnyWindowHandle>);
impl Global for SettingsWindow {}

/// Opens the settings window, or brings the open one to the front. Returns the window and its
/// view when a new window was opened.
pub fn open(
    prefs: Entity<Prefs>,
    os: Os,
    open_path: fn(&Path),
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<SettingsView>)> {
    // Keylens has no Dock icon; without activating, the window cannot take keyboard focus.
    cx.activate(true);
    if let Some(handle) = cx.try_global::<SettingsWindow>().and_then(|w| w.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return None;
    }
    // Bind once: the global exists from the first opening on.
    if !cx.has_global::<SettingsWindow>() {
        cx.bind_keys([KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-w"
            },
            CloseSettings,
            Some(KEY_CONTEXT),
        )]);
    }
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(t!("Settings.title").to_string().into()),
            ..Default::default()
        }),
        kind: WindowKind::Normal,
        focus: true,
        show: true,
        is_resizable: false,
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(480.), px(336.)),
            cx,
        ))),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| SettingsView::new(prefs, os, open_path, window, cx))
    }) {
        Ok((handle, view)) => {
            // `focus: true` is not enough for an Accessory app's window to become key everywhere.
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            cx.set_global(SettingsWindow(Some(handle)));
            Some((handle, view))
        }
        Err(error) => {
            tracing::warn!(%error, "cannot open the settings window");
            None
        }
    }
}

/// The value to save for what is typed in a duration field: only whole numbers inside `range`.
/// Anything else (an empty field, or `12` on the way to `1200`) waits for more typing.
pub fn committed_ms(text: &str, range: &RangeInclusive<u64>) -> Option<u64> {
    text.trim()
        .parse::<u64>()
        .ok()
        .filter(|ms| range.contains(ms))
}

pub fn modifier_label(key: Modifiers, os: Os) -> &'static str {
    match (key, os) {
        (Modifiers::META, Os::MacOS) => "⌘ Command",
        (Modifiers::ALT, Os::MacOS) => "⌥ Option",
        (Modifiers::CTRL, Os::MacOS) => "⌃ Control",
        (Modifiers::SHIFT, Os::MacOS) => "⇧ Shift",
        (Modifiers::META, Os::Windows) => "Win",
        (Modifiers::META, _) => "Super",
        (Modifiers::ALT, _) => "Alt",
        (Modifiers::CTRL, _) => "Ctrl",
        _ => "Shift",
    }
}

pub struct SettingsView {
    prefs: Entity<Prefs>,
    os: Os,
    open_path: fn(&Path),
    hold: Entity<InputState>,
    tap: Entity<InputState>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(
        prefs: Entity<Prefs>,
        os: Os,
        open_path: fn(&Path),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = prefs.read(cx).trigger_settings();
        let hold = number_state(current.hold_ms, &HOLD_MS, HOLD_STEP, window, cx);
        let tap = number_state(
            current.double_tap_ms,
            &DOUBLE_TAP_MS,
            DOUBLE_TAP_STEP,
            window,
            cx,
        );
        let subscriptions = vec![
            subscribe_ms(&hold, HOLD_MS, |ms| Change::HoldMs(Some(ms)), window, cx),
            subscribe_ms(
                &tap,
                DOUBLE_TAP_MS,
                |ms| Change::DoubleTapMs(Some(ms)),
                window,
                cx,
            ),
            cx.observe_in(&prefs, window, |this, _, window, cx| {
                this.sync_inputs(window, cx);
                window.set_window_title(&t!("Settings.title"));
                cx.notify();
            }),
            cx.observe_window_appearance(window, |_, window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
            }),
            cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    cx.emit(ChildDeactivated);
                }
            }),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            prefs,
            os,
            open_path,
            hold,
            tap,
            focus,
            _subscriptions: subscriptions,
        }
    }

    /// Shows the saved durations, unless a field already shows that value (e.g. while typing).
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.prefs.read(cx).trigger_settings();
        for (input, ms) in [
            (self.hold.clone(), current.hold_ms),
            (self.tap.clone(), current.double_tap_ms),
        ] {
            let shown = input.read(cx).value().trim().parse::<u64>().ok();
            if shown != Some(ms) {
                input.update(cx, |input, cx| input.set_value(ms.to_string(), window, cx));
            }
        }
    }

    fn set(&self, change: Change, cx: &mut App) {
        self.prefs.update(cx, |p, cx| p.set(change, cx));
    }

    fn render_problem(&self, problem: Problem, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, can_open) = match problem {
            Problem::Invalid(error) => (t!("Settings.invalid", error = error).to_string(), true),
            Problem::WriteFailed(error) => (
                t!("Settings.write_failed", error = error).to_string(),
                false,
            ),
        };
        let path = self.prefs.read(cx).path().to_path_buf();
        let open_path = self.open_path;
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
                    Button::new("open-config")
                        .small()
                        .label(t!("Settings.open_file"))
                        .on_click(move |_, _, _| open_path(&path)),
                )
            })
    }
}

fn number_state(
    value: u64,
    range: &RangeInclusive<u64>,
    step: u64,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .default_value(value.to_string())
            .step(step as f64)
            .min(*range.start() as f64)
            .max(*range.end() as f64)
    })
}

/// Saves a duration field whenever it holds a committable value (see `committed_ms`). The input
/// clamps out-of-range text on blur, which arrives here as one more change.
fn subscribe_ms(
    input: &Entity<InputState>,
    range: RangeInclusive<u64>,
    change: fn(u64) -> Change,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Subscription {
    cx.subscribe_in(
        input,
        window,
        move |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => {
                if let Some(ms) = committed_ms(&input.read(cx).value(), &range) {
                    this.set(change(ms), cx);
                }
            }
            // An empty or unparseable field was never saved; snap back to the effective value.
            InputEvent::Blur => this.sync_inputs(window, cx),
            _ => {}
        },
    )
}

/// A row inside a section card: the label on the left, then a fixed slot for the restore button
/// (shown while the value is not the default), then the control.
fn row(
    id: &'static str,
    label: impl Into<SharedString>,
    control: impl IntoElement,
    reset: Option<Change>,
    prefs: &Entity<Prefs>,
    read_only: bool,
) -> AnyElement {
    let prefs = prefs.clone();
    h_flex()
        .w_full()
        .min_h(px(40.))
        .px_3()
        .gap_2()
        .child(div().flex_1().min_w_0().child(label.into()))
        .child(
            h_flex()
                .w(px(RESET_SLOT))
                .justify_center()
                .when_some(reset, move |this, change| {
                    this.child(
                        Button::new(SharedString::from(format!("{id}-reset")))
                            .ghost()
                            .xsmall()
                            .label("↺")
                            .tooltip(t!("Settings.reset"))
                            .disabled(read_only)
                            .on_click(move |_, _, cx| {
                                let change = change.clone();
                                prefs.update(cx, |p, cx| p.set(change, cx));
                            }),
                    )
                }),
        )
        .child(div().w(px(CONTROL_WIDTH)).child(control))
        .into_any_element()
}

/// A dropdown button listing `options`, with `current` checked.
fn dropdown<T: Clone + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(T, SharedString)>,
    current: T,
    fallback_label: SharedString,
    to_change: fn(T) -> Change,
    prefs: &Entity<Prefs>,
    read_only: bool,
) -> impl IntoElement {
    let label = options
        .iter()
        .find(|(value, _)| *value == current)
        .map(|(_, label)| label.clone())
        .unwrap_or(fallback_label);
    let prefs = prefs.clone();
    Button::new(id)
        .w_full()
        .small()
        .label(label)
        .dropdown_caret(true)
        .outline()
        .disabled(read_only)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            options.iter().fold(menu, |menu, (value, label)| {
                let prefs = prefs.clone();
                let picked = value.clone();
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(*value == current)
                        .on_click(move |_, _, cx| {
                            let change = to_change(picked.clone());
                            prefs.update(cx, |p, cx| p.set(change, cx));
                        }),
                )
            })
        })
}

impl EventEmitter<ChildDeactivated> for SettingsView {}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prefs = self.prefs.read(cx);
        let read_only = prefs.read_only();
        let problem = prefs.problem().cloned();
        let current = prefs.trigger_settings();
        let defaults = prefs.default_trigger_settings();
        let language = prefs.config().language.clone();
        let everything_default = current == defaults && language.is_none();
        let os = self.os;

        let modes = vec![
            (
                TriggerMode::Hold,
                t!("Settings.mode_hold").to_string().into(),
            ),
            (
                TriggerMode::DoubleTap,
                t!("Settings.mode_double_tap").to_string().into(),
            ),
        ];
        let keys = Modifiers::TRIGGER_KEYS
            .iter()
            .map(|k| (*k, SharedString::from(modifier_label(*k, os))))
            .collect();
        let mut languages: Vec<(Option<String>, SharedString)> =
            vec![(None, t!("Settings.language_system").to_string().into())];
        languages.extend(
            LANGUAGES
                .iter()
                .map(|(tag, name)| (Some(tag.to_string()), (*name).into())),
        );
        let language_fallback: SharedString = language.clone().unwrap_or_default().into();

        let (delay_label, delay_input, delay_reset, delay_note) = match current.mode {
            TriggerMode::Hold => (
                t!("Settings.hold_ms"),
                self.hold.clone(),
                (current.hold_ms != defaults.hold_ms).then_some(Change::HoldMs(None)),
                t!("Settings.hold_note"),
            ),
            TriggerMode::DoubleTap => (
                t!("Settings.double_tap_ms"),
                self.tap.clone(),
                (current.double_tap_ms != defaults.double_tap_ms)
                    .then_some(Change::DoubleTapMs(None)),
                t!("Settings.double_tap_note"),
            ),
        };

        let mut trigger_rows = vec![
            row(
                "mode",
                t!("Settings.mode").to_string(),
                dropdown(
                    "mode-select",
                    modes,
                    current.mode,
                    SharedString::default(),
                    |m| Change::Mode(Some(m)),
                    &self.prefs,
                    read_only,
                ),
                (current.mode != defaults.mode).then_some(Change::Mode(None)),
                &self.prefs,
                read_only,
            ),
            row(
                "key",
                t!("Settings.key").to_string(),
                dropdown(
                    "key-select",
                    keys,
                    current.key,
                    SharedString::default(),
                    |k| Change::Key(Some(k)),
                    &self.prefs,
                    read_only,
                ),
                (current.key != defaults.key).then_some(Change::Key(None)),
                &self.prefs,
                read_only,
            ),
            row(
                "delay",
                delay_label.to_string(),
                NumberInput::new(&delay_input)
                    .small()
                    .disabled(read_only)
                    .suffix(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("ms"),
                    ),
                delay_reset,
                &self.prefs,
                read_only,
            ),
        ];
        // Releasing the key ends a double-tap at once, so the option only means something for hold.
        if current.mode == TriggerMode::Hold {
            let prefs = self.prefs.clone();
            trigger_rows.push(row(
                "hide-on-release",
                t!("Settings.hide_on_release").to_string(),
                h_flex().justify_end().child(
                    Switch::new("hide-on-release-switch")
                        .checked(current.hide_on_release)
                        .disabled(read_only)
                        .on_click(move |checked, _, cx| {
                            let change = Change::HideOnRelease(Some(*checked));
                            prefs.update(cx, |p, cx| p.set(change, cx));
                        }),
                ),
                (current.hide_on_release != defaults.hide_on_release)
                    .then_some(Change::HideOnRelease(None)),
                &self.prefs,
                read_only,
            ));
        }
        let language_rows = vec![row(
            "language",
            t!("Settings.language").to_string(),
            dropdown(
                "language-select",
                languages,
                language.clone(),
                language_fallback,
                Change::Language,
                &self.prefs,
                read_only,
            ),
            language.is_some().then_some(Change::Language(None)),
            &self.prefs,
            read_only,
        )];

        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(|_: &CloseSettings, window, _| window.remove_window())
            .size_full()
            .px_4()
            .pt_3()
            .pb_4()
            .gap_6()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .when_some(problem, |this, problem| {
                this.child(self.render_problem(problem, cx))
            })
            .child(section(
                t!("Settings.trigger_group").to_string(),
                trigger_rows,
                Some(delay_note.to_string().into()),
                cx,
            ))
            .child(section(
                t!("Settings.ui_group").to_string(),
                language_rows,
                None,
                cx,
            ))
            .child(
                // The ghost button pads its label by 8 px; 4 px more puts the label on the same
                // start line as the section titles.
                h_flex().w_full().px_1().child(
                    Button::new("reset-all")
                        .ghost()
                        .small()
                        .label(t!("Settings.reset_all"))
                        .disabled(read_only || everything_default)
                        .on_click(cx.listener(|this, _, _, cx| this.set(Change::ResetAll, cx))),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    // See view.rs: keep the built-in #[test] in scope next to gpui's.
    use core::prelude::v1::test;

    use gpui_kit::TestAppContext;
    use keylens_core::Trigger;

    use super::*;
    use crate::config::HOLD_MS;

    #[test]
    fn only_in_range_numbers_are_committed() {
        assert_eq!(committed_ms("1200", &HOLD_MS), Some(1200));
        assert_eq!(committed_ms(" 300 ", &HOLD_MS), Some(300));
        // Intermediate keystrokes while typing 1200.
        assert_eq!(committed_ms("1", &HOLD_MS), None);
        assert_eq!(committed_ms("12", &HOLD_MS), None);
        assert_eq!(committed_ms("120", &HOLD_MS), None);
        assert_eq!(committed_ms("", &HOLD_MS), None);
        assert_eq!(committed_ms("-5", &HOLD_MS), None);
        assert_eq!(committed_ms("9000", &HOLD_MS), None);
        assert_eq!(committed_ms("800.5", &HOLD_MS), None);
    }

    #[test]
    fn modifier_labels_use_platform_names() {
        assert_eq!(modifier_label(Modifiers::META, Os::MacOS), "⌘ Command");
        assert_eq!(modifier_label(Modifiers::ALT, Os::MacOS), "⌥ Option");
        assert_eq!(modifier_label(Modifiers::META, Os::Linux), "Super");
        assert_eq!(modifier_label(Modifiers::CTRL, Os::Linux), "Ctrl");
    }

    fn prefs(cx: &mut TestAppContext, name: &str) -> Entity<Prefs> {
        let dir = std::env::temp_dir().join(format!("keylens-sv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let default = Trigger::HoldModifier {
            key: Modifiers::META,
            delay: Trigger::DEFAULT_HOLD_DELAY,
        };
        let p = Prefs::load(dir.join("config.toml"), vec![], default, Os::MacOS);
        cx.new(|_| p)
    }

    #[gpui_kit::test]
    fn opening_twice_keeps_one_window(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let prefs = prefs(cx, "twice");
        cx.update(|cx| {
            open(prefs.clone(), Os::MacOS, |_| {}, cx);
            open(prefs.clone(), Os::MacOS, |_| {}, cx);
        });
        assert_eq!(cx.windows().len(), 1);
    }

    #[gpui_kit::test]
    fn number_fields_follow_external_changes(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let prefs = prefs(cx, "sync");
        let window = cx.add_window(|window, cx| {
            SettingsView::new(prefs.clone(), Os::MacOS, |_| {}, window, cx)
        });
        prefs.update(cx, |p, cx| p.set(Change::HoldMs(Some(1500)), cx));
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert_eq!(view.hold.read(cx).value().to_string(), "1500");
            })
            .unwrap();
    }

    #[gpui_kit::test]
    fn close_shortcut_works_without_clicking(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let prefs = prefs(cx, "close");
        cx.update(|cx| open(prefs.clone(), Os::MacOS, |_| {}, cx));
        assert_eq!(cx.windows().len(), 1);
        let handle = cx.windows()[0];
        cx.simulate_keystrokes(
            handle,
            if cfg!(target_os = "macos") {
                "cmd-w"
            } else {
                "ctrl-w"
            },
        );
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 0);
    }

    #[gpui_kit::test]
    fn blur_restores_cleared_duration_field(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let prefs = prefs(cx, "blur");
        let window = cx.add_window(|window, cx| {
            SettingsView::new(prefs.clone(), Os::MacOS, |_| {}, window, cx)
        });
        let effective = prefs.read_with(cx, |p, _| p.trigger_settings().hold_ms);
        window
            .update(cx, |view, window, cx| {
                view.hold
                    .update(cx, |input, cx| input.set_value("", window, cx));
                assert_eq!(view.hold.read(cx).value().to_string(), "");
                view.hold.update(cx, |_, cx| cx.emit(InputEvent::Blur));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert_eq!(
                    view.hold.read(cx).value().to_string(),
                    effective.to_string()
                );
            })
            .unwrap();
    }
}
