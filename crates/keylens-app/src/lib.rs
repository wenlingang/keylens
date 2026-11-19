//! Keylens app: the panel UI and control logic.
// Only `macos` talks to AppKit and needs `unsafe`; everything else must not.
#![deny(unsafe_code)]

rust_i18n::i18n!("locales", fallback = "en");

pub mod accent;
pub mod charge;
pub mod config;
pub mod config_write;
pub mod controller;
pub mod loader;
pub mod logging;
#[allow(unsafe_code)]
pub mod macos;
pub mod model;
pub mod permission;
pub mod platform;
pub mod prefs;
pub mod recorder;
pub mod session;
pub mod settings_view;
pub mod shortcut_editor;
pub mod strings;
pub mod template;
pub mod ui;
pub mod update;
pub mod updater;
pub mod user_data_edit;
pub mod view;
pub mod watch;

use std::rc::Rc;
use std::sync::Arc;

use crate::macos::tray::{Tray, TrayCommand, TrayLabels};
use crate::updater::UpdateNative;

/// Bundle identifier of released builds; any other bundle (or none) is a development build.
pub const RELEASE_BUNDLE_ID: &str = "io.github.wenlingang.keylens";

/// Version line shown in the menu bar menu, e.g. "Keylens 0.1.0" or "Keylens 0.1.0 (dev)".
/// Not translated.
pub fn version_label(dev: bool) -> String {
    let suffix = if dev { " (dev)" } else { "" };
    format!("Keylens {}{suffix}", env!("CARGO_PKG_VERSION"))
}

/// Tray labels in the current UI language. Development builds get a distinct menu bar icon, so
/// they are easy to tell apart from an installed release running at the same time.
fn tray_labels(dev: bool) -> TrayLabels {
    use rust_i18n::t;
    TrayLabels {
        icon: if dev { "⌘dev" } else { "⌘" }.to_string(),
        version: version_label(dev),
        permission_ok: t!("Tray.permission_ok").to_string(),
        permission_missing: t!("Tray.permission_missing").to_string(),
        edit_current: t!("Tray.edit_current").to_string(),
        open_data: t!("Tray.open_data").to_string(),
        settings: t!("Tray.settings").to_string(),
        check_updates: t!("Tray.check_updates").to_string(),
        quit: t!("Tray.quit").to_string(),
    }
}

/// Starts Keylens.
pub fn run() {
    use gpui_kit::component::Theme;
    use gpui_kit::*;
    use keylens_core::community::{builtin, load_dir};
    use rust_i18n::t;

    use crate::charge;
    use crate::controller::Keylens;
    use crate::loader::Loader;
    // Named explicitly: gpui_kit::* also exports a `Platform`.
    use crate::platform::Platform;
    use crate::updater::Updater;
    use crate::view::{PanelView, bind_keys};

    struct KeylensApp {
        _keylens: Entity<Keylens>,
        _updater: Entity<Updater>,
        _tray: Rc<Tray>,
        _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    }
    impl Global for KeylensApp {}

    let platform: Arc<dyn Platform> = Arc::new(macos::MacPlatform::default());
    let native = controller::Native {
        hide: |w| macos::panel::hide(w),
        raise: |w, raised| macos::panel::set_raised(w, raised),
        fade_in: |w| macos::panel::fade_in(w),
        show_overlay: |w, pid| macos::panel::show_overlay(w, pid),
        reduce_motion: macos::panel::reduce_motion,
        set_alpha: macos::panel::set_alpha,
        place: |w, pid| macos::panel::place(w, pid),
        beep: macos::panel::beep,
        open: macos::panel::open_path,
    };
    let update = UpdateNative {
        install: macos::update::install,
        relaunch: macos::update::relaunch,
        dialog: macos::update::dialog,
    };
    // A development build (not the released bundle): separate logs, no automatic updates.
    let dev = macos::panel::bundle_identifier().as_deref() != Some(RELEASE_BUNDLE_ID);

    let log_guard = logging::init(&logging::log_dir(dev));
    tracing::info!(version = env!("CARGO_PKG_VERSION"), dev, "Keylens starting");

    let prefs_state = prefs::Prefs::load(
        config::config_path(),
        sys_locale::get_locales().collect(),
        platform.default_trigger(),
    );
    let locale = prefs_state.locale();
    rust_i18n::set_locale(locale.ui);

    let (builtin_catalog, builtin_errors) = builtin();
    let (user_catalog, user_errors) = load_dir(&config::user_data_dir());
    for error in builtin_errors.iter().chain(&user_errors) {
        tracing::warn!("data file error: {error}");
    }

    let loader = Arc::new(Loader::new(
        platform.clone(),
        Arc::new(builtin_catalog),
        Arc::new(user_catalog),
        locale.lang,
    ));

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let prefs = cx.new(|_| prefs_state);
            cx.set_activation_policy(ActivationPolicy::Accessory);
            cx.set_quit_mode(QuitMode::Explicit);
            bind_keys(cx);

            let visible = cx.primary_display().map(|d| d.visible_bounds().size);
            let (width, height) = visible
                .map(|s| {
                    (
                        (f32::from(s.width) - 32.0).min(1040.0),
                        (f32::from(s.height) * 0.72).min(640.0),
                    )
                })
                .unwrap_or((1040.0, 640.0));
            let options = WindowOptions {
                kind: WindowKind::PopUp,
                titlebar: None,
                focus: false,
                show: false,
                is_resizable: false,
                is_movable: false,
                window_background: WindowBackgroundAppearance::Blurred,
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(width), px(height)),
                    cx,
                ))),
                ..Default::default()
            };
            let (window, view) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| PanelView::new(window, cx))
            })
            .expect("failed to create the panel window");
            let _ = window.update(cx, |_, window, cx| {
                Theme::sync_system_appearance(Some(window), cx)
            });

            // The hold animation's window: transparent, never focused, shown only while the
            // trigger key is held (see `Keylens::start_charge`).
            let charge_options = WindowOptions {
                kind: WindowKind::PopUp,
                titlebar: None,
                focus: false,
                show: false,
                is_resizable: false,
                is_movable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                // Never focused, so gpui would cap it at 30 fps like any inactive window; the
                // animation only asks for frames while it plays.
                inactive_frame_interval: None,
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(charge::WINDOW_SIZE), px(charge::WINDOW_SIZE)),
                    cx,
                ))),
                ..Default::default()
            };
            // Opened directly, not through gpui_kit::open_window: its root layer is for windows
            // with controls, and this one must stay see-through around the keycap.
            let charge_window = cx
                .open_window(charge_options, |_, cx| {
                    cx.new(|_| charge::ChargeView::new(native.set_alpha))
                })
                .map(|handle| {
                    let view = handle.update(cx, |_, _, cx| cx.entity());
                    (AnyWindowHandle::from(handle), view)
                });

            let keylens = cx.new(|cx| {
                Keylens::new(
                    platform.clone(),
                    loader.clone(),
                    native,
                    prefs.clone(),
                    window,
                    view,
                    cx,
                )
            });
            match charge_window {
                Ok((window, Ok(view))) => keylens.update(cx, |k, _| k.attach_charge(window, view)),
                Ok((_, Err(error))) | Err(error) => {
                    tracing::warn!("could not create the hold animation window: {error}")
                }
            }

            let (tray_tx, tray_rx) = async_channel::unbounded::<TrayCommand>();
            let labels = tray_labels(dev);
            let mtm = objc2::MainThreadMarker::new().expect("must run on the main thread");
            let tray = Rc::new(Tray::install(
                mtm,
                labels,
                Box::new(move |command| {
                    let _ = tray_tx.try_send(command);
                }),
            ));

            let tray_for_updates = tray.clone();
            let updater = cx.new(|cx| {
                let mut updater = Updater::new(
                    Arc::new(update::Curl),
                    update,
                    prefs.clone(),
                    Box::new(move |status| tray_for_updates.set_update_status(status)),
                );
                // Dev builds are signed with the development certificate: an update would not verify.
                if !dev {
                    updater.start(cx);
                } else if let Ok(version) = std::env::var("KEYLENS_PRETEND_VERSION") {
                    // Lets a dev build show the update dialog and progress (see CONTRIBUTING.md).
                    updater.pretend_version(version);
                }
                updater
            });

            let keylens_for_tray = keylens.clone();
            let updater_for_tray = updater.clone();
            cx.spawn(async move |cx| {
                while let Ok(command) = tray_rx.recv().await {
                    cx.update(|cx| match command {
                        TrayCommand::Permission => {
                            keylens_for_tray.update(cx, |k, _| k.request_permission())
                        }
                        TrayCommand::EditCurrentApp => {
                            keylens_for_tray.update(cx, |k, cx| k.edit_current_app(cx))
                        }
                        TrayCommand::OpenDataFolder => {
                            let dir = config::user_data_dir();
                            let _ = std::fs::create_dir_all(&dir);
                            (native.open)(&dir);
                        }
                        TrayCommand::Settings => {
                            keylens_for_tray.update(cx, |k, cx| k.open_settings(cx))
                        }
                        TrayCommand::CheckForUpdates => {
                            updater_for_tray.update(cx, |u, cx| u.check(true, cx))
                        }
                        TrayCommand::Quit => cx.quit(),
                    });
                }
            })
            .detach();

            let tray_for_errors = tray.clone();
            keylens.update(cx, |k, _| {
                k.set_data_errors_listener(
                    Box::new(move |count| {
                        let text =
                            (count > 0).then(|| t!("Tray.data_errors", count = count).to_string());
                        tray_for_errors.set_data_errors(text.as_deref());
                    }),
                    builtin_errors.len(),
                    user_errors.len(),
                )
            });
            let tray_for_permission = tray.clone();
            keylens.update(cx, |k, _| {
                k.set_permission_listener(Box::new(move |granted| {
                    tray_for_permission.set_permission(granted)
                }))
            });
            let tray_for_locale = tray.clone();
            keylens.update(cx, |k, _| {
                k.set_locale_listener(Box::new(move || {
                    tray_for_locale.set_labels(tray_labels(dev))
                }))
            });
            keylens.update(cx, |k, cx| k.start(cx));
            cx.set_global(KeylensApp {
                _keylens: keylens,
                _updater: updater,
                _tray: tray,
                _log_guard: log_guard,
            });
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_label_names_the_app_and_package_version() {
        assert_eq!(
            super::version_label(false),
            format!("Keylens {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            super::version_label(true),
            format!("Keylens {} (dev)", env!("CARGO_PKG_VERSION"))
        );
    }
}
