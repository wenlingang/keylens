//! Keylens controller: ties together key events, frontmost-app changes, background reads and panel display (spec §5).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use keylens_core::{
    AppInfo, AppShortcuts, Entry, KeyEvent, Trigger, TriggerAction, TriggerDetector,
};
use keylens_platform::{ListenGuard, PermissionState, Platform};

use crate::loader::{Cache, Freshness, Loader};
use crate::permission::{PermissionAction, PermissionWatch};
use crate::prefs::Prefs;
use crate::session::{Effect, Session};
use crate::settings_view;
use crate::shortcut_editor;
use crate::ui::ChildDeactivated;
use crate::view::{PanelEvent, PanelView};
use crate::{config, template, watch};
use keylens_core::community::{Catalog, DataError, load_dir};

/// Number of cached apps (spec §5).
const CACHE_CAPACITY: usize = 8;
const PERMISSION_POLL: Duration = Duration::from_secs(1);
/// How often the user data folder is checked for edits.
const USER_DATA_POLL: Duration = Duration::from_secs(2);
/// How long to wait after a Keylens window loses focus before checking where focus went: AppKit
/// reports the old window resigning before the new one becoming key.
const FOCUS_SETTLE: Duration = Duration::from_millis(100);

/// A deferred panel move returned by [`Native::place`].
pub type PanelMove = Box<dyn FnOnce()>;

/// Platform-specific operations the controller needs; replaceable in tests.
#[derive(Clone, Copy)]
pub struct Native {
    pub hide: fn(&Window),
    /// Puts the panel above ordinary windows (`true`, its usual pop-up level) or among them
    /// (`false`), so the settings window can come in front of it.
    pub raise: fn(&Window, bool),
    /// Works out how to move the panel onto the screen showing the app with this pid (or under the
    /// mouse). The returned move must run outside any window update.
    pub place: fn(&Window, Option<i32>) -> Option<PanelMove>,
    pub beep: fn(),
    /// Opens a file or folder with its default app.
    pub open: fn(&std::path::Path),
}

pub struct Keylens {
    platform: Arc<dyn Platform>,
    loader: Arc<Loader>,
    native: Native,
    prefs: Entity<Prefs>,
    /// The trigger in effect; `detector` is rebuilt from it.
    trigger: Trigger,
    /// The UI language in effect (a rust-i18n locale such as "zh-CN").
    ui_locale: &'static str,
    /// User data files with errors, from the last reload (shown in the tray with the built-in ones).
    user_errors: usize,
    on_locale: Option<Box<dyn Fn()>>,
    cache: Cache,
    freshness: Freshness,
    icons: HashMap<i32, Option<Arc<Image>>>,
    detector: TriggerDetector,
    window: AnyWindowHandle,
    view: Entity<PanelView>,
    session: Session,
    /// Keylens' own windows besides the panel (settings, shortcut editor) while open. The panel
    /// stays up alongside them, one level lower.
    children: Vec<AnyWindowHandle>,
    child_subscriptions: Vec<(AnyWindowHandle, Vec<Subscription>)>,
    focus_check: Option<Task<()>>,
    tick: Option<Task<()>>,
    permission_task: Option<Task<()>>,
    user_data_task: Option<Task<()>>,
    on_permission: Option<Box<dyn Fn(bool)>>,
    /// Reports the number of data files with errors (built-in plus user).
    on_data_errors: Option<Box<dyn Fn(usize)>>,
    builtin_errors: usize,
    guards: Vec<ListenGuard>,
    /// Whether the key listener is running (it can fail to start right after a grant).
    listening: bool,
    _subscriptions: Vec<Subscription>,
}

impl Keylens {
    pub fn new(
        platform: Arc<dyn Platform>,
        loader: Arc<Loader>,
        native: Native,
        prefs: Entity<Prefs>,
        window: AnyWindowHandle,
        view: Entity<PanelView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let on_panel = cx.subscribe(&view, |this, _, event: &PanelEvent, cx| {
            this.on_panel_event(event.clone(), cx)
        });
        let on_prefs = cx.observe(&prefs, |this, _, cx| this.on_prefs_changed(cx));
        let (trigger, ui_locale) = {
            let p = prefs.read(cx);
            (p.trigger_settings().trigger(), p.locale().ui)
        };
        let detector = TriggerDetector::new(trigger.clone());
        Self {
            platform,
            loader,
            native,
            prefs,
            trigger,
            ui_locale,
            user_errors: 0,
            on_locale: None,
            cache: Cache::new(CACHE_CAPACITY),
            freshness: Freshness::default(),
            icons: HashMap::new(),
            detector,
            window,
            view,
            session: Session::new(std::process::id() as i32),
            children: Vec::new(),
            child_subscriptions: Vec::new(),
            focus_check: None,
            tick: None,
            permission_task: None,
            user_data_task: None,
            on_permission: None,
            on_data_errors: None,
            builtin_errors: 0,
            guards: Vec::new(),
            listening: false,
            _subscriptions: vec![on_panel, on_prefs],
        }
    }

    /// Callback invoked when the status bar item etc. needs to know about permission changes.
    /// Callback for the number of data files with errors; called right away with
    /// `builtin_errors + user_errors` and again after every reload of the user data.
    pub fn set_data_errors_listener(
        &mut self,
        listener: Box<dyn Fn(usize)>,
        builtin_errors: usize,
        user_errors: usize,
    ) {
        self.user_errors = user_errors;
        listener(builtin_errors + user_errors);
        self.builtin_errors = builtin_errors;
        self.on_data_errors = Some(listener);
    }

    pub fn set_permission_listener(&mut self, listener: Box<dyn Fn(bool)>) {
        self.on_permission = Some(listener);
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.watch_user_data(cx);
        self.permission_task = Some(cx.spawn(async move |this, cx| {
            let mut watch = PermissionWatch::default();
            loop {
                // The controller is gone: stop watching.
                let Ok(actions) = this.update(cx, |this, _| {
                    let granted = this.platform.permission() == PermissionState::Granted;
                    watch.step(granted, this.listening)
                }) else {
                    break;
                };
                for action in actions {
                    if this
                        .update(cx, |this, cx| this.on_permission_action(action, cx))
                        .is_err()
                    {
                        return;
                    }
                }
                cx.background_executor().timer(PERMISSION_POLL).await;
            }
        }));
    }

    fn on_permission_action(&mut self, action: PermissionAction, cx: &mut Context<Self>) {
        match action {
            PermissionAction::Revoked => {
                tracing::info!("accessibility permission missing or revoked");
                self.stop_listening();
                // A pending long-press must not show an app panel over the guide.
                self.detector = TriggerDetector::new(self.trigger.clone());
                self.tick = None;
                self.notify_permission(false);
                self.show_permission_page(cx);
            }
            PermissionAction::StartListening => {
                // Fresh listeners: an event tap created before the permission was granted
                // (or before it was revoked) never receives events again.
                self.stop_listening();
                if self.start_listening(cx) {
                    tracing::info!("accessibility permission granted; listening");
                    self.notify_permission(true);
                    if self.session.visible() && self.session.target().is_none() {
                        self.hide(cx);
                    }
                }
            }
        }
    }

    fn stop_listening(&mut self) {
        self.guards.clear();
        self.listening = false;
    }

    /// The menu bar icon reflects whether Keylens actually listens, not just the permission flag.
    fn notify_permission(&self, ok: bool) {
        if let Some(listener) = &self.on_permission {
            listener(ok);
        }
    }

    /// Reloads the user data whenever a `.toml` file in the user data folder changes, so edits
    /// take effect without restarting.
    fn watch_user_data(&mut self, cx: &mut Context<Self>) {
        self.user_data_task = Some(cx.spawn(async move |this, cx| {
            let dir = config::user_data_dir();
            let mut last = watch::fingerprint(&dir);
            loop {
                cx.background_executor().timer(USER_DATA_POLL).await;
                let scan = dir.clone();
                let (now, loaded) = cx
                    .background_executor()
                    .spawn(async move {
                        let now = watch::fingerprint(&scan);
                        let loaded = load_dir(&scan);
                        (now, loaded)
                    })
                    .await;
                if now == last {
                    continue;
                }
                last = now;
                let (catalog, errors) = loaded;
                if this
                    .update(cx, |this, cx| {
                        this.on_user_data_changed(catalog, errors, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn on_user_data_changed(
        &mut self,
        catalog: Catalog,
        errors: Vec<DataError>,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(errors = errors.len(), "user shortcut data reloaded");
        for error in &errors {
            tracing::warn!("data file error: {error}");
        }
        self.user_errors = errors.len();
        if let Some(listener) = &self.on_data_errors {
            listener(self.builtin_errors + self.user_errors);
        }
        self.loader.set_user_catalog(Arc::new(catalog));
        // Cached shortcuts were built from the old user data.
        self.cache = Cache::new(CACHE_CAPACITY);
        if self.session.visible()
            && let Some(app) = self.session.target().cloned()
        {
            self.prefetch(app, cx);
        }
    }

    /// Edits the shortcuts of the frontmost app (from the menu bar icon).
    pub fn edit_current_app(&mut self, cx: &mut Context<Self>) {
        let own = std::process::id() as i32;
        match self.platform.frontmost_app().filter(|a| a.pid != own) {
            Some(app) => self.open_editor(app, cx),
            None => (self.native.beep)(),
        }
    }

    pub fn request_permission(&mut self) {
        self.platform.request_permission();
    }

    // ---- Event sources ----

    pub fn trigger(&self) -> &Trigger {
        &self.trigger
    }

    /// Called after the UI language changed, e.g. so the tray can rebuild its labels.
    pub fn set_locale_listener(&mut self, listener: Box<dyn Fn()>) {
        self.on_locale = Some(listener);
    }

    /// Applies the trigger and language from `Prefs` (settings window or a hand edit).
    fn on_prefs_changed(&mut self, cx: &mut Context<Self>) {
        let (trigger, locale) = {
            let p = self.prefs.read(cx);
            (p.trigger_settings().trigger(), p.locale())
        };
        if trigger != self.trigger {
            tracing::info!(?trigger, "trigger changed");
            // A press in progress belongs to the old trigger: drop it rather than finish it.
            self.detector = TriggerDetector::new(trigger.clone());
            self.trigger = trigger;
            self.tick = None;
        }
        if locale.ui != self.ui_locale {
            tracing::info!(locale = locale.ui, "UI language changed");
            self.ui_locale = locale.ui;
            rust_i18n::set_locale(locale.ui);
            self.loader.set_lang(locale.lang);
            // Cached shortcuts carry titles in the old language.
            self.cache = Cache::new(CACHE_CAPACITY);
            let view = self.view.clone();
            let _ = self.window.update(cx, |_, window, cx| {
                view.update(cx, |view, cx| view.refresh_locale(window, cx))
            });
            if self.session.visible()
                && let Some(app) = self.session.target().cloned()
            {
                self.prefetch(app, cx);
            }
            if let Some(listener) = &self.on_locale {
                listener();
            }
            if let Some(listener) = &self.on_data_errors {
                listener(self.builtin_errors + self.user_errors);
            }
        }
    }

    /// Starts the key listener and the frontmost-app watcher. Returns whether the key listener
    /// started; on failure the permission watch retries on its next check.
    fn start_listening(&mut self, cx: &mut Context<Self>) -> bool {
        let (key_tx, key_rx) = std::sync::mpsc::channel();
        match self.platform.start_key_events(key_tx) {
            Ok(guard) => self.guards.push(guard),
            Err(error) => {
                tracing::warn!(%error, "cannot start the key listener; will retry");
                return false;
            }
        }
        self.listening = true;
        let keys = bridge(key_rx);
        cx.spawn(async move |this, cx| {
            while let Ok(event) = keys.recv().await {
                if this
                    .update(cx, |this, cx| this.on_key_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let (app_tx, app_rx) = std::sync::mpsc::channel();
        if let Ok(guard) = self.platform.watch_frontmost(app_tx) {
            self.guards.push(guard);
            let apps = bridge(app_rx);
            cx.spawn(async move |this, cx| {
                while let Ok(app) = apps.recv().await {
                    if this
                        .update(cx, |this, cx| this.on_frontmost(app, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }
        true
    }

    // ---- Trigger ----

    fn on_key_event(&mut self, event: KeyEvent, cx: &mut Context<Self>) {
        let now = Instant::now();
        if let Some(action) = self.detector.on_event(event, now) {
            self.on_trigger(action, cx);
        }
        self.schedule_tick(cx);
    }

    fn schedule_tick(&mut self, cx: &mut Context<Self>) {
        let Some(deadline) = self.detector.next_deadline() else {
            self.tick = None;
            return;
        };
        let delay = deadline.saturating_duration_since(Instant::now());
        self.tick = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(action) = this.detector.on_tick(Instant::now()) {
                    this.on_trigger(action, cx);
                }
                this.schedule_tick(cx);
            });
        }));
    }

    fn on_trigger(&mut self, action: TriggerAction, cx: &mut Context<Self>) {
        let effects = match action {
            TriggerAction::Prefetch => self.session.on_prefetch(self.platform.frontmost_app()),
            TriggerAction::Show => self.session.on_show(self.platform.frontmost_app()),
            TriggerAction::Cancel => {
                self.session.on_cancel();
                Vec::new()
            }
        };
        self.apply(effects, cx);
    }

    fn on_frontmost(&mut self, app: AppInfo, cx: &mut Context<Self>) {
        let effects = self.session.on_frontmost(app);
        self.apply(effects, cx);
    }

    fn apply(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                Effect::Prefetch(app) => self.prefetch(app, cx),
                Effect::Present(app) => self.show(app, cx),
                Effect::Hide => self.hide(cx),
            }
        }
    }

    // ---- Reading ----

    fn prefetch(&mut self, app: AppInfo, cx: &mut Context<Self>) {
        let loader = self.loader.clone();
        let platform = self.platform.clone();
        let need_icon = !self.icons.contains_key(&app.pid);
        cx.spawn(async move |this, cx| {
            let (data, icon) = cx
                .background_executor()
                .spawn(async move {
                    let icon = if need_icon {
                        Some(platform.app_icon(&app))
                    } else {
                        None
                    };
                    (loader.load(&app), icon)
                })
                .await;
            let _ = this.update(cx, |this, cx| this.on_loaded(data, icon, cx));
        })
        .detach();
    }

    fn on_loaded(
        &mut self,
        data: AppShortcuts,
        icon: Option<Option<Vec<u8>>>,
        cx: &mut Context<Self>,
    ) {
        let pid = data.app.pid;
        if !data.warnings.is_empty() {
            tracing::warn!(app = %data.app.name, warnings = ?data.warnings, "menu read failed");
        }
        if let Some(icon) = icon {
            self.icons.insert(
                pid,
                icon.map(|png| Arc::new(Image::from_bytes(ImageFormat::Png, png))),
            );
        }
        // Out-of-order or failed reads must not replace newer/good data (see `Freshness`).
        if !self.freshness.accept(&data) {
            return;
        }
        self.cache.put(data.clone());
        if self.session.shows(pid) {
            let icon = self.icons.get(&pid).cloned().flatten();
            self.view
                .update(cx, |view, cx| view.set_data(data, icon, cx));
        }
    }

    // ---- Show and hide ----

    /// Shows the panel for `app` (the session already made it the target).
    fn show(&mut self, app: AppInfo, cx: &mut Context<Self>) {
        let icon = self.icons.get(&app.pid).cloned().flatten();
        let cached = self.cache.get(app.pid).cloned();
        let has_cache = cached.is_some();
        self.view.update(cx, |view, cx| match cached {
            Some(data) => view.set_data(data, icon, cx),
            None => view.set_loading(app.clone(), icon, cx),
        });
        self.present(cx);
        if has_cache {
            // Show the cache first while refreshing in the background (menu item availability changes at any time).
            self.prefetch(app, cx);
        }
    }

    fn show_permission_page(&mut self, cx: &mut Context<Self>) {
        self.session.on_permission_page();
        self.view.update(cx, |view, cx| view.set_no_permission(cx));
        self.present(cx);
    }

    fn present(&mut self, cx: &mut Context<Self>) {
        let window = self.window;
        let place = self.native.place;
        let raise = self.native.raise;
        // Below Keylens' other windows while any is open, so clicking one brings it in front.
        let raised = self.children.is_empty();
        let pid = self.session.target().map(|t| t.pid);
        // Placing the panel has to happen while the App is not borrowed (see `Native::place`), so
        // it runs from a task, between two updates.
        cx.spawn(async move |this, cx| {
            if let Ok(Some(apply)) = window.update(cx, |_, window, _| {
                raise(window, raised);
                place(window, pid)
            }) {
                apply();
            }
            let _ = this.update(cx, |this, cx| {
                // The panel may have been dismissed before this task ran.
                if !this.session.visible() {
                    return;
                }
                let view = this.view.clone();
                let _ = this.window.update(cx, |_, window, cx| {
                    window.activate_window();
                    view.update(cx, |view, cx| view.focus_input(window, cx));
                });
            });
        })
        .detach();
    }

    fn hide(&mut self, cx: &mut Context<Self>) {
        if !self.session.visible() {
            return;
        }
        self.session.on_hidden();
        let view = self.view.clone();
        let hide = self.native.hide;
        let _ = self.window.update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.reset(window, cx));
            hide(window);
        });
    }

    // ---- Panel events ----

    fn on_panel_event(&mut self, event: PanelEvent, cx: &mut Context<Self>) {
        match event {
            PanelEvent::Execute(entry) => self.execute(entry, cx),
            PanelEvent::Dismiss => self.hide(cx),
            PanelEvent::Deactivated => self.on_window_deactivated(cx),
            PanelEvent::OpenPermissionSettings => self.platform.request_permission(),
            PanelEvent::OpenSettings => self.open_settings(cx),
            PanelEvent::EditShortcuts(app) => self.open_editor(app, cx),
        }
    }

    // ---- Settings window ----

    /// Opens the settings window, or brings it to the front. An open panel stays, one level lower
    /// so the settings window can sit in front of it.
    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.lower_panel_for_child(cx);
        let prefs = self.prefs.clone();
        let open_path = self.native.open;
        // Open after this update: the panel's window may still be borrowed here.
        cx.spawn(async move |this, cx| {
            let opened = cx.update(|cx| {
                settings_view::open(prefs, keylens_core::Os::current(), open_path, cx)
            });
            if let Some((handle, view)) = opened {
                let _ = this.update(cx, |this, cx| this.on_child_opened(handle, view, cx));
            }
        })
        .detach();
    }

    /// Opens the shortcut editor for `app`, or points the open one at it. The panel stays.
    pub fn open_editor(&mut self, app: AppInfo, cx: &mut Context<Self>) {
        if template::user_data_template(&app).is_none() {
            (self.native.beep)();
            return;
        }
        self.lower_panel_for_child(cx);
        let open_path = self.native.open;
        let beep = self.native.beep;
        // Open after this update: the panel's window may still be borrowed here.
        cx.spawn(async move |this, cx| {
            let opened = cx.update(|cx| {
                shortcut_editor::open(app, keylens_core::Os::current(), open_path, beep, cx)
            });
            if let Some((handle, view)) = opened {
                let _ = this.update(cx, |this, cx| this.on_child_opened(handle, view, cx));
            }
        })
        .detach();
    }

    /// Lowers the visible panel so a Keylens child window can sit in front of it.
    fn lower_panel_for_child(&mut self, cx: &mut Context<Self>) {
        if self.session.visible() {
            self.set_panel_raised(false, cx);
        }
    }

    fn on_child_opened<V: EventEmitter<ChildDeactivated>>(
        &mut self,
        handle: AnyWindowHandle,
        view: Entity<V>,
        cx: &mut Context<Self>,
    ) {
        if self.children.contains(&handle) {
            return;
        }
        self.children.push(handle);
        let subscriptions = vec![
            cx.subscribe(&view, |this, _, _: &ChildDeactivated, cx| {
                this.on_window_deactivated(cx)
            }),
            cx.observe_release(&view, move |this, _, cx| this.on_child_closed(handle, cx)),
        ];
        self.child_subscriptions.push((handle, subscriptions));
    }

    /// Once the last of Keylens' other windows closes, puts the panel back on top and gives it
    /// focus again, so Esc and clicking away work as usual.
    fn on_child_closed(&mut self, handle: AnyWindowHandle, cx: &mut Context<Self>) {
        self.children.retain(|h| *h != handle);
        self.child_subscriptions.retain(|(h, _)| *h != handle);
        if !self.children.is_empty() || !self.session.visible() {
            return;
        }
        self.set_panel_raised(true, cx);
        let view = self.view.clone();
        let _ = self.window.update(cx, |_, window, cx| {
            window.activate_window();
            view.update(cx, |view, cx| view.focus_input(window, cx));
        });
    }

    fn set_panel_raised(&self, raised: bool, cx: &mut Context<Self>) {
        let raise = self.native.raise;
        let _ = self.window.update(cx, |_, window, _| raise(window, raised));
    }

    /// The panel or a child window lost focus. Without a child window the panel closes right
    /// away; with one, it closes only once focus has left Keylens altogether.
    fn on_window_deactivated(&mut self, cx: &mut Context<Self>) {
        if self.children.is_empty() {
            self.hide(cx);
            return;
        }
        self.focus_check = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FOCUS_SETTLE).await;
            let _ = this.update(cx, |this, cx| {
                if !this.has_focus(cx) {
                    this.hide(cx);
                }
            });
        }));
    }

    /// Whether the panel or any of Keylens' other windows has keyboard focus.
    fn has_focus(&self, cx: &mut Context<Self>) -> bool {
        let active = |handle: AnyWindowHandle, cx: &mut Context<Self>| {
            handle
                .update(cx, |_, window, _| window.is_window_active())
                .unwrap_or(false)
        };
        active(self.window, cx) || self.children.iter().any(|h| active(*h, cx))
    }

    fn execute(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.hide(cx);
        let platform = self.platform.clone();
        let beep = self.native.beep;
        let title = entry.title.clone();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { platform.execute(&entry) })
                .await;
            if let Err(error) = result {
                tracing::warn!(entry = %title, %error, "execute failed");
                cx.update(|_| beep());
            }
        })
        .detach();
    }
}

/// Turns a std channel into an async channel: a background thread blocks on receive and forwards to gpui's async loop.
fn bridge<T: Send + 'static>(rx: std::sync::mpsc::Receiver<T>) -> async_channel::Receiver<T> {
    let (tx, async_rx) = async_channel::unbounded();
    std::thread::spawn(move || {
        while let Ok(value) = rx.recv() {
            if tx.send_blocking(value).is_err() {
                break;
            }
        }
    });
    async_rx
}

#[cfg(test)]
mod tests {
    // See view.rs: keep the built-in #[test] in scope next to gpui's.
    use core::prelude::v1::test;

    use gpui_kit::{TestAppContext, VisualTestContext};
    use keylens_core::community::Catalog;
    use keylens_core::{Modifiers, Os};
    use keylens_platform::fake::FakePlatform;

    use super::*;
    use crate::config::TriggerMode;
    use crate::config_write::Change;
    use crate::prefs::Prefs;

    fn setup(cx: &mut TestAppContext) -> (Entity<Keylens>, Entity<Prefs>) {
        setup_with(cx, || {}, Arc::default())
    }

    fn setup_with(
        cx: &mut TestAppContext,
        beep: fn(),
        fake: Arc<FakePlatform>,
    ) -> (Entity<Keylens>, Entity<Prefs>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::view::bind_keys(cx);
        });
        let platform: Arc<dyn Platform> = fake;
        let loader = Arc::new(Loader::new(
            platform.clone(),
            Arc::new(Catalog::default()),
            Arc::new(Catalog::default()),
            Os::MacOS,
            keylens_core::Lang::En,
        ));
        let dir = std::env::temp_dir().join(format!("keylens-ctl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prefs = Prefs::load(
            dir.join("config.toml"),
            vec![],
            platform.default_trigger(),
            Os::MacOS,
        );
        let prefs = cx.new(|_| prefs);
        let window = cx.add_window(|window, cx| PanelView::new(Os::MacOS, window, cx));
        let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        let native = Native {
            hide: |_| {},
            raise: |_, _| {},
            place: |_, _| None,
            beep,
            open: |_| {},
        };
        let keylens = cx.new(|cx| {
            Keylens::new(
                platform,
                loader,
                native,
                prefs.clone(),
                window.into(),
                view,
                cx,
            )
        });
        (keylens, prefs)
    }

    fn show_panel(keylens: &Entity<Keylens>, cx: &mut TestAppContext) {
        let app = AppInfo {
            pid: 42,
            name: "Demo".into(),
            ids: vec![],
        };
        keylens.update(cx, |k, cx| {
            let effects = k.session.on_show(Some(app));
            k.apply(effects, cx);
        });
        cx.run_until_parked();
    }

    fn panel_visible(keylens: &Entity<Keylens>, cx: &mut TestAppContext) -> bool {
        keylens.read_with(cx, |k, _| k.session.visible())
    }

    /// Opens the settings window from the panel; returns its handle.
    fn open_settings_from_panel(
        keylens: &Entity<Keylens>,
        cx: &mut TestAppContext,
    ) -> AnyWindowHandle {
        keylens.update(cx, |k, cx| k.on_panel_event(PanelEvent::OpenSettings, cx));
        cx.run_until_parked();
        keylens
            .read_with(cx, |k, _| k.children.first().copied())
            .expect("the settings window is open")
    }

    #[gpui_kit::test]
    fn editing_the_current_app_beeps_when_keylens_itself_is_frontmost(cx: &mut TestAppContext) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static BEEPS: AtomicUsize = AtomicUsize::new(0);
        let fake = Arc::new(FakePlatform::default());
        let (keylens, _) = setup_with(
            cx,
            || {
                BEEPS.fetch_add(1, Ordering::SeqCst);
            },
            fake.clone(),
        );
        fake.set_frontmost(Some(AppInfo {
            pid: std::process::id() as i32,
            name: "Keylens".into(),
            ids: vec!["com.keylens.app".into()],
        }));
        keylens.update(cx, |k, cx| k.edit_current_app(cx));
        assert_eq!(BEEPS.load(Ordering::SeqCst), 1);
    }

    #[gpui_kit::test]
    fn opening_settings_keeps_the_panel(cx: &mut TestAppContext) {
        let (keylens, _) = setup(cx);
        show_panel(&keylens, cx);
        open_settings_from_panel(&keylens, cx);
        assert_eq!(cx.windows().len(), 2);
        assert!(panel_visible(&keylens, cx));
    }

    #[gpui_kit::test]
    fn panel_closes_only_when_focus_leaves_keylens(cx: &mut TestAppContext) {
        let (keylens, _) = setup(cx);
        show_panel(&keylens, cx);
        let settings = open_settings_from_panel(&keylens, cx);

        // Opening the settings window moved focus away from the panel: the panel stays.
        assert_eq!(cx.update(|cx| cx.active_window()), Some(settings));
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert!(panel_visible(&keylens, cx));

        // Focus left the settings window for another app: the panel goes too.
        VisualTestContext::from_window(settings, cx).deactivate_window();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert!(!panel_visible(&keylens, cx));
    }

    #[gpui_kit::test]
    fn closing_settings_gives_focus_back_to_the_panel(cx: &mut TestAppContext) {
        let (keylens, _) = setup(cx);
        show_panel(&keylens, cx);
        let settings = open_settings_from_panel(&keylens, cx);
        settings
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        let panel = keylens.read_with(cx, |k, _| k.window);
        assert_eq!(cx.update(|cx| cx.active_window()), Some(panel));
        assert!(panel_visible(&keylens, cx));
        assert!(keylens.read_with(cx, |k, _| k.children.is_empty()));
    }

    /// A bare child window, standing in for a second kind of Keylens window.
    struct TestChild;

    impl EventEmitter<ChildDeactivated> for TestChild {}

    impl Render for TestChild {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui_kit::test]
    fn panel_refocuses_only_after_the_last_child_closes(cx: &mut TestAppContext) {
        let (keylens, _) = setup(cx);
        show_panel(&keylens, cx);
        let settings = open_settings_from_panel(&keylens, cx);
        let second = cx.add_window(|_, _| TestChild);
        let view = second.update(cx, |_, _, cx| cx.entity()).unwrap();
        keylens.update(cx, |k, cx| k.on_child_opened(second.into(), view, cx));
        assert_eq!(keylens.read_with(cx, |k, _| k.children.len()), 2);
        let panel = keylens.read_with(cx, |k, _| k.window);

        second
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert_eq!(keylens.read_with(cx, |k, _| k.children.len()), 1);
        assert_ne!(cx.update(|cx| cx.active_window()), Some(panel));
        assert!(panel_visible(&keylens, cx));

        settings
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert!(keylens.read_with(cx, |k, _| k.children.is_empty()));
        assert_eq!(cx.update(|cx| cx.active_window()), Some(panel));
    }

    #[gpui_kit::test]
    fn without_settings_the_panel_closes_when_it_loses_focus(cx: &mut TestAppContext) {
        let (keylens, _) = setup(cx);
        show_panel(&keylens, cx);
        keylens.update(cx, |k, cx| k.on_panel_event(PanelEvent::Deactivated, cx));
        assert!(!panel_visible(&keylens, cx));
    }

    #[gpui_kit::test]
    fn trigger_follows_prefs(cx: &mut TestAppContext) {
        let (keylens, prefs) = setup(cx);
        prefs.update(cx, |p, cx| {
            p.set(Change::Mode(Some(TriggerMode::DoubleTap)), cx);
            p.set(Change::Key(Some(Modifiers::ALT)), cx);
        });
        cx.run_until_parked();
        keylens.read_with(cx, |k, _| {
            assert_eq!(
                k.trigger(),
                &Trigger::DoubleTap {
                    key: Modifiers::ALT,
                    interval: Trigger::DEFAULT_DOUBLE_TAP_INTERVAL,
                }
            );
            assert_eq!(k.detector.trigger(), k.trigger());
        });
    }
}
