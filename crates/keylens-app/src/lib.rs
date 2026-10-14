//! Keylens app: the panel UI and control logic.
#![forbid(unsafe_code)]

rust_i18n::i18n!("locales", fallback = "en");

pub mod config;
pub mod controller;
pub mod loader;
pub mod logging;
pub mod model;
pub mod permission;
pub mod session;
pub mod strings;
pub mod template;
pub mod view;
pub mod watch;

/// Starts Keylens (macOS only).
#[cfg(target_os = "macos")]
pub fn run() {
    use std::sync::Arc;

    use gpui_kit::component::Theme;
    use gpui_kit::*;
    use keylens_core::Os;
    use keylens_core::community::{builtin, load_dir};
    use keylens_macos::tray::{Tray, TrayCommand, TrayLabels};
    use keylens_platform::Platform;
    use rust_i18n::t;

    use crate::controller::{Keylens, Native};
    use crate::loader::Loader;
    use crate::view::{PanelView, bind_keys};

    fn objc2_main_thread() -> keylens_macos::MainThreadMarker {
        keylens_macos::MainThreadMarker::new().expect("must run on the main thread")
    }

    struct KeylensApp {
        _keylens: Entity<Keylens>,
        _tray: std::rc::Rc<Tray>,
        _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    }
    impl Global for KeylensApp {}

    let log_guard = logging::init(&logging::log_dir());
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Keylens starting");

    let (config, config_error) = config::load_config(&config::config_path());
    if let Some(error) = config_error {
        tracing::warn!("config file has errors, using defaults: {error}");
    }
    let system: Vec<String> = sys_locale::get_locales().collect();
    let locale = config::choose_locale(&config, &system);
    rust_i18n::set_locale(locale.ui);

    let os = Os::current();
    let (builtin_catalog, builtin_errors) = builtin();
    let (user_catalog, user_errors) = load_dir(&config::user_data_dir());
    for error in builtin_errors.iter().chain(&user_errors) {
        tracing::warn!("data file error: {error}");
    }

    let platform: Arc<dyn Platform> = Arc::new(keylens_macos::MacPlatform::default());
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

            let native = Native {
                hide: |w| keylens_macos::panel::hide(w),
                place: |w, pid| keylens_macos::panel::place(w, pid),
                beep: keylens_macos::panel::beep,
                open: keylens_macos::panel::open_path,
            };
            let keylens = cx
                .new(|cx| Keylens::new(platform.clone(), loader.clone(), native, window, view, cx));

            let (tray_tx, tray_rx) = async_channel::unbounded::<TrayCommand>();
            let mtm = objc2_main_thread();
            let labels = TrayLabels {
                permission_ok: t!("Tray.permission_ok").to_string(),
                permission_missing: t!("Tray.permission_missing").to_string(),
                edit_current: t!("Tray.edit_current").to_string(),
                open_data: t!("Tray.open_data").to_string(),
                quit: t!("Tray.quit").to_string(),
            };
            let tray = std::rc::Rc::new(Tray::install(
                mtm,
                labels,
                Box::new(move |command| {
                    let _ = tray_tx.try_send(command);
                }),
            ));

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
                            keylens_macos::panel::open_path(&dir);
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
            keylens.update(cx, |k, cx| k.start(cx));
            cx.set_global(KeylensApp {
                _keylens: keylens,
                _tray: tray,
                _log_guard: log_guard,
            });
        });
}
