//! Keylens controller: ties together key events, frontmost-app changes, background reads and panel display (spec §5).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use keylens_core::{AppInfo, AppShortcuts, Entry, KeyEvent, TriggerAction, TriggerDetector};
use keylens_platform::{ListenGuard, PermissionState, Platform};

use crate::loader::{Cache, Freshness, Loader};
use crate::permission::{PermissionAction, PermissionWatch};
use crate::session::{Effect, Session};
use crate::view::{PanelEvent, PanelView};
use crate::{config, template, watch};
use keylens_core::community::{Catalog, DataError, load_dir};

/// Number of cached apps (spec §5).
const CACHE_CAPACITY: usize = 8;
const PERMISSION_POLL: Duration = Duration::from_secs(1);
/// How often the user data folder is checked for edits.
const USER_DATA_POLL: Duration = Duration::from_secs(2);

/// Platform-specific operations the controller needs; replaceable in tests.
pub struct Native {
    pub hide: fn(&Window),
    /// Moves the panel onto the screen showing the app with this pid (or under the mouse).
    pub place: fn(&Window, Option<i32>),
    pub beep: fn(),
    /// Opens a file or folder with its default app.
    pub open: fn(&std::path::Path),
}

pub struct Keylens {
    platform: Arc<dyn Platform>,
    loader: Arc<Loader>,
    native: Native,
    cache: Cache,
    freshness: Freshness,
    icons: HashMap<i32, Option<Arc<Image>>>,
    detector: TriggerDetector,
    window: AnyWindowHandle,
    view: Entity<PanelView>,
    session: Session,
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
        window: AnyWindowHandle,
        view: Entity<PanelView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let on_panel = cx.subscribe(&view, |this, _, event: &PanelEvent, cx| {
            this.on_panel_event(event.clone(), cx)
        });
        let detector = TriggerDetector::new(platform.default_trigger());
        Self {
            platform,
            loader,
            native,
            cache: Cache::new(CACHE_CAPACITY),
            freshness: Freshness::default(),
            icons: HashMap::new(),
            detector,
            window,
            view,
            session: Session::new(std::process::id() as i32),
            tick: None,
            permission_task: None,
            user_data_task: None,
            on_permission: None,
            on_data_errors: None,
            builtin_errors: 0,
            guards: Vec::new(),
            listening: false,
            _subscriptions: vec![on_panel],
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
                self.detector = TriggerDetector::new(self.platform.default_trigger());
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
        if let Some(listener) = &self.on_data_errors {
            listener(self.builtin_errors + errors.len());
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
        if let Some(app) = self.platform.frontmost_app().filter(|a| a.pid != own) {
            self.edit_shortcuts(app, cx);
        }
    }

    /// Creates (if missing) and opens the user data file for `app`. Never overwrites what the
    /// user has already written (see `create_user_file`).
    fn edit_shortcuts(&mut self, app: AppInfo, cx: &mut Context<Self>) {
        self.hide(cx);
        match template::create_user_file(&config::user_data_dir(), &app) {
            Ok(Some(path)) => (self.native.open)(&path),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, "cannot create the user data file");
                (self.native.beep)();
            }
        }
    }

    pub fn request_permission(&mut self) {
        self.platform.request_permission();
    }

    // ---- Event sources ----

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
        let view = self.view.clone();
        let place = self.native.place;
        let pid = self.session.target().map(|t| t.pid);
        let _ = self.window.update(cx, |_, window, cx| {
            place(window, pid);
            window.activate_window();
            view.update(cx, |view, cx| view.focus_input(window, cx));
        });
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
            PanelEvent::OpenSettings => self.platform.request_permission(),
            PanelEvent::EditShortcuts(app) => self.edit_shortcuts(app, cx),
        }
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
