//! Keylens controller: ties together key events, frontmost-app changes, background reads and panel display (spec §5).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use keylens_core::{AppInfo, AppShortcuts, Entry, KeyEvent, TriggerAction, TriggerDetector};
use keylens_platform::{ListenGuard, PermissionState, Platform};

use crate::loader::{Cache, Freshness, Loader};
use crate::session::{Effect, Session};
use crate::view::{PanelEvent, PanelView};

/// Number of cached apps (spec §5).
const CACHE_CAPACITY: usize = 8;
const PERMISSION_POLL: Duration = Duration::from_secs(1);

/// Platform-specific operations the controller needs; replaceable in tests.
pub struct Native {
    pub hide: fn(&Window),
    /// Moves the panel onto the screen showing the app with this pid (or under the mouse).
    pub place: fn(&Window, Option<i32>),
    pub beep: fn(),
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
    permission_poll: Option<Task<()>>,
    on_permission: Option<Box<dyn Fn(bool)>>,
    guards: Vec<ListenGuard>,
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
            permission_poll: None,
            on_permission: None,
            guards: Vec::new(),
            _subscriptions: vec![on_panel],
        }
    }

    /// Callback invoked when the status bar item etc. needs to know about permission changes.
    pub fn set_permission_listener(&mut self, listener: Box<dyn Fn(bool)>) {
        self.on_permission = Some(listener);
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.platform.permission() == PermissionState::Granted {
            self.start_listening(cx);
        } else {
            self.show_permission_page(cx);
            self.poll_permission(cx);
        }
    }

    pub fn request_permission(&mut self) {
        self.platform.request_permission();
    }

    // ---- Event sources ----

    fn start_listening(&mut self, cx: &mut Context<Self>) {
        let (key_tx, key_rx) = std::sync::mpsc::channel();
        match self.platform.start_key_events(key_tx) {
            Ok(guard) => self.guards.push(guard),
            Err(_) => {
                self.poll_permission(cx);
                return;
            }
        }
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
        if let Some(listener) = &self.on_permission {
            listener(true);
        }
    }

    fn poll_permission(&mut self, cx: &mut Context<Self>) {
        if let Some(listener) = &self.on_permission {
            listener(false);
        }
        self.permission_poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PERMISSION_POLL).await;
                let granted = this
                    .update(cx, |this, _| {
                        this.platform.permission() == PermissionState::Granted
                    })
                    .unwrap_or(false);
                if granted {
                    let _ = this.update(cx, |this, cx| {
                        this.permission_poll = None;
                        this.start_listening(cx);
                        this.hide(cx);
                    });
                    break;
                }
            }
        }));
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
