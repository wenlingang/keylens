//! Keylens app: the panel UI and control logic.
#![forbid(unsafe_code)]

rust_i18n::i18n!("locales", fallback = "en");

pub mod config;
pub mod config_write;
pub mod controller;
pub mod loader;
pub mod logging;
pub mod model;
pub mod permission;
pub mod prefs;
pub mod recorder;
pub mod session;
pub mod settings_view;
pub mod shortcut_editor;
pub mod strings;
pub mod template;
pub mod ui;
pub mod user_data_edit;
pub mod view;
pub mod watch;

use std::rc::Rc;
use std::sync::Arc;

use keylens_platform::Platform;
use keylens_platform::tray::{TrayCommand, TrayHandle, TrayLabels};

/// Version line shown in the menu bar menu, e.g. "Keylens 0.1.0". Not translated.
pub fn version_label() -> String {
    format!("Keylens {}", env!("CARGO_PKG_VERSION"))
}

/// Tray labels in the current UI language.
fn tray_labels() -> TrayLabels {
    use rust_i18n::t;
    TrayLabels {
        version: version_label(),
        permission_ok: t!("Tray.permission_ok").to_string(),
        permission_missing: t!("Tray.permission_missing").to_string(),
        edit_current: t!("Tray.edit_current").to_string(),
        open_data: t!("Tray.open_data").to_string(),
        settings: t!("Tray.settings").to_string(),
        quit: t!("Tray.quit").to_string(),
    }
}

/// Installs the tray, forwarding menu clicks to the callback. Called on the main thread once the
/// UI is running.
pub type InstallTray = fn(TrayLabels, Box<dyn Fn(TrayCommand)>) -> Rc<dyn TrayHandle>;

/// The platform-specific pieces `run_with` needs; everything else in startup is shared.
pub struct Host {
    pub platform: Arc<dyn Platform>,
    pub native: controller::Native,
    pub install_tray: InstallTray,
}

/// Starts Keylens (macOS only).
#[cfg(target_os = "macos")]
pub fn run() {
    fn install_tray(
        labels: TrayLabels,
        on_command: Box<dyn Fn(TrayCommand)>,
    ) -> Rc<dyn TrayHandle> {
        let mtm = keylens_macos::MainThreadMarker::new().expect("must run on the main thread");
        Rc::new(keylens_macos::tray::Tray::install(mtm, labels, on_command))
    }

    run_with(Host {
        platform: Arc::new(keylens_macos::MacPlatform::default()),
        native: controller::Native {
            hide: |w| keylens_macos::panel::hide(w),
            raise: |w, raised| keylens_macos::panel::set_raised(w, raised),
            place: |w, pid| keylens_macos::panel::place(w, pid),
            beep: keylens_macos::panel::beep,
            open: keylens_macos::panel::open_path,
        },
        install_tray,
    });
}

/// Starts Keylens on top of the given platform pieces. Contains no platform-specific code, so it
/// compiles on every target; a new platform only has to provide a [`Host`].
pub fn run_with(host: Host) {
    use gpui_kit::component::Theme;
    use gpui_kit::*;
    use keylens_core::Os;
    use keylens_core::community::{builtin, load_dir};
    use rust_i18n::t;

    use crate::controller::Keylens;
    use crate::loader::Loader;
    use crate::view::{PanelView, bind_keys};

    struct KeylensApp {
        _keylens: Entity<Keylens>,
        _tray: Rc<dyn TrayHandle>,
        _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    }
    impl Global for KeylensApp {}

    let Host {
        platform,
        native,
        install_tray,
    } = host;

    let log_guard = logging::init(&logging::log_dir());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Keylens starting");

    let prefs_state = prefs::Prefs::load(
        config::config_path(),
        sys_locale::get_locales().collect(),
        platform.default_trigger(),
        keylens_core::Os::current(),
    );
    let locale = prefs_state.locale();
    rust_i18n::set_locale(locale.ui);

    let os = Os::current();
    let (builtin_catalog, builtin_errors) = builtin();
    let (user_catalog, user_errors) = load_dir(&config::user_data_dir());
    for error in builtin_errors.iter().chain(&user_errors) {
        tracing::warn!("data file error: {error}");
    }

    let loader = Arc::new(Loader::new(
        platform.clone(),
        Arc::new(builtin_catalog),
        Arc::new(user_catalog),
        os,
        locale.lang,
    ));

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let prefs = cx.new(|cx| {
                let mut prefs = prefs_state;
                prefs.watch(cx);
                prefs
            });
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
                cx.new(|cx| PanelView::new(os, window, cx))
            })
            .expect("failed to create the panel window");
            let _ = window.update(cx, |_, window, cx| {
                Theme::sync_system_appearance(Some(window), cx)
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

            let (tray_tx, tray_rx) = async_channel::unbounded::<TrayCommand>();
            let labels = tray_labels();
            let tray = install_tray(
                labels,
                Box::new(move |command| {
                    let _ = tray_tx.try_send(command);
                }),
            );

            let keylens_for_tray = keylens.clone();
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
                k.set_locale_listener(Box::new(move || tray_for_locale.set_labels(tray_labels())))
            });
            keylens.update(cx, |k, cx| k.start(cx));
            cx.set_global(KeylensApp {
                _keylens: keylens,
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
            super::version_label(),
            format!("Keylens {}", env!("CARGO_PKG_VERSION"))
        );
    }
}
