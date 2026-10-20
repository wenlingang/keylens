//! Windows probe: hold Ctrl in any app → a window lists that app's community shortcuts → Enter
//! hands focus back and sends the shortcut. Everything worth knowing goes to the log file.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod foreground;
#[cfg(windows)]
mod hook;

#[cfg(not(windows))]
fn main() {
    eprintln!("windows-probe only runs on Windows");
}

/// Appends a timestamped line to %LOCALAPPDATA%\KeylensProbe\probe.log.
#[cfg(windows)]
pub fn log(line: &str) {
    use std::io::Write;
    let Some(base) = std::env::var_os("LOCALAPPDATA") else { return };
    let dir = std::path::Path::new(&base).join("KeylensProbe");
    let _ = std::fs::create_dir_all(&dir);
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("probe.log")) {
        let _ = writeln!(f, "{}.{:03} {line}", since.as_secs(), since.subsec_millis());
    }
}

#[cfg(windows)]
fn main() {
    app::run();
}

#[cfg(windows)]
mod app {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use gpui_kit::component::input::{Input, InputEvent, InputState};
    use gpui_kit::component::*;
    use gpui_kit::*;
    use keylens_core::community::{Catalog, CommunitySource, builtin};
    use keylens_core::{
        AppInfo, Entry, EntrySource, KeyEvent, Lang, Modifiers, Os, ShortcutSource, Trigger,
        TriggerAction, TriggerDetector,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use crate::foreground::{self, Hwnd};
    use crate::log;

    /// The app that was in front when the trigger fired.
    #[derive(Clone)]
    struct Target {
        hwnd: Hwnd,
        exe: String,
    }

    struct Popup {
        input: Entity<InputState>,
        entries: Vec<Entry>,
        query: String,
        selected: usize,
        target: Target,
        status: String,
        _subscriptions: Vec<Subscription>,
    }

    impl Popup {
        fn new(target: Target, entries: Vec<Entry>, window: &mut Window, cx: &mut Context<Self>) -> Self {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search shortcuts…"));
            window.focus(&input.focus_handle(cx));
            let change = cx.subscribe_in(&input, window, |this: &mut Self, input, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::Change => {
                        this.query = input.read(cx).value().to_string();
                        this.selected = 0;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.execute(this.selected, window),
                    _ => {}
                }
            });
            let keys = cx.observe_keystrokes(|this: &mut Self, event, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => this.close(window),
                    "down" => {
                        this.selected = (this.selected + 1).min(this.visible().len().saturating_sub(1));
                        cx.notify();
                    }
                    "up" => {
                        this.selected = this.selected.saturating_sub(1);
                        cx.notify();
                    }
                    _ => {}
                }
            });

            // Native position/focus checks run once the window exists on screen.
            let ours = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get() as Hwnd),
                _ => None,
            };
            let near = target.hwnd;
            let status = format!("{} · {} shortcuts", target.exe, entries.len());
            log(&format!("popup for {status}"));
            if let Some(ours) = ours {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(120));
                    let focused = foreground::present(ours, near);
                    log(&format!("popup focused={focused}"));
                });
            }
            Self {
                input,
                entries,
                query: String::new(),
                selected: 0,
                target,
                status,
                _subscriptions: vec![change, keys],
            }
        }

        fn visible(&self) -> Vec<&Entry> {
            let q = self.query.to_lowercase();
            self.entries
                .iter()
                .filter(|e| q.is_empty() || e.title.to_lowercase().contains(&q) || e.group.to_lowercase().contains(&q))
                .collect()
        }

        fn close(&mut self, window: &mut Window) {
            let hwnd = self.target.hwnd;
            window.remove_window();
            std::thread::spawn(move || {
                let ok = foreground::restore(hwnd);
                log(&format!("escape: focus restored={ok}"));
            });
        }

        fn execute(&mut self, index: usize, window: &mut Window) {
            let Some(entry) = self.visible().get(index).map(|e| (*e).clone()) else { return };
            let hwnd = self.target.hwnd;
            window.remove_window();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                let restored = foreground::restore(hwnd);
                let sent = restored && foreground::send(&entry.shortcut);
                log(&format!(
                    "execute '{}' ({}): restored={restored} sent={sent}",
                    entry.title,
                    entry.shortcut.display(Os::Windows)
                ));
            });
        }
    }

    impl Render for Popup {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let selected = self.selected;
            let rows: Vec<AnyElement> = self
                .visible()
                .into_iter()
                .enumerate()
                .take(200)
                .map(|(i, e)| {
                    h_flex()
                        .id(("row", i))
                        .px_2()
                        .py_1()
                        .gap_2()
                        .rounded(px(4.))
                        .when(i == selected, |d| d.bg(theme.accent))
                        .child(div().flex_1().child(e.title.clone()))
                        .child(div().text_color(theme.muted_foreground).child(e.group.clone()))
                        .child(div().child(e.shortcut.display(Os::Windows)))
                        .on_click(cx.listener(move |this, ev: &ClickEvent, window, _| {
                            if ev.click_count() >= 2 {
                                this.execute(i, window);
                            } else {
                                this.selected = i;
                            }
                        }))
                        .into_any_element()
                })
                .collect();
            v_flex()
                .size_full()
                .p_3()
                .gap_2()
                .bg(theme.background)
                .text_color(theme.foreground)
                .child(div().text_sm().text_color(theme.muted_foreground).child(self.status.clone()))
                .child(Input::new(&self.input))
                .child(v_flex().id("rows").flex_1().overflow_y_scroll().children(rows))
        }
    }

    struct Status;

    impl Render for Status {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            v_flex()
                .size_full()
                .p_4()
                .gap_2()
                .bg(theme.background)
                .text_color(theme.foreground)
                .child("Keylens Windows probe is running.")
                .child("Hold Ctrl for about 0.8s in any app. Close this window to quit.")
                .child(div().text_sm().text_color(theme.muted_foreground).child("Log: %LOCALAPPDATA%\\KeylensProbe\\probe.log"))
        }
    }

    /// Runs the trigger detector on its own thread, forwarding only Show to the UI.
    fn detect(events: std::sync::mpsc::Receiver<KeyEvent>, shows: async_channel::Sender<()>) {
        std::thread::spawn(move || {
            let mut detector = TriggerDetector::new(Trigger::HoldModifier {
                key: Modifiers::CTRL,
                delay: Trigger::DEFAULT_HOLD_DELAY,
            });
            loop {
                let event = match detector.next_deadline() {
                    Some(deadline) => {
                        let wait = deadline.saturating_duration_since(Instant::now());
                        match events.recv_timeout(wait) {
                            Ok(e) => Some(e),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                            Err(_) => break,
                        }
                    }
                    None => match events.recv() {
                        Ok(e) => Some(e),
                        Err(_) => break,
                    },
                };
                let action = match event {
                    Some(e) => detector.on_event(e, Instant::now()),
                    None => detector.on_tick(Instant::now()),
                };
                if action == Some(TriggerAction::Show) {
                    log("trigger: show");
                    if shows.send_blocking(()).is_err() {
                        break;
                    }
                }
            }
        });
    }

    fn show(catalog: &Arc<Catalog>, cx: &mut App) {
        if cx.windows().len() > 1 {
            return; // a popup is already open
        }
        let Some((hwnd, pid, exe)) = foreground::current() else {
            log("trigger: no foreground window");
            return;
        };
        let app = AppInfo { pid: pid as i32, name: exe.clone(), ids: vec![exe.clone()] };
        let source = CommunitySource::new(catalog.clone(), Os::Windows, Lang::En, EntrySource::Community);
        let entries = source.load(&app).unwrap_or_default();
        let target = Target { hwnd, exe };
        let options = WindowOptions {
            kind: WindowKind::PopUp,
            titlebar: None,
            focus: true,
            window_bounds: Some(WindowBounds::centered(size(px(640.), px(460.)), cx)),
            ..Default::default()
        };
        if let Err(e) = gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| Popup::new(target, entries, window, cx))
        }) {
            log(&format!("open popup failed: {e:?}"));
        }
    }

    pub fn run() {
        log("probe starting");
        let (catalog, errors) = builtin();
        log(&format!("builtin catalog: {} apps, {} errors", catalog.len(), errors.len()));
        let catalog = Arc::new(catalog);

        let (key_tx, key_rx) = std::sync::mpsc::channel();
        let (show_tx, show_rx) = async_channel::unbounded();
        crate::hook::start(key_tx);
        detect(key_rx, show_tx);

        gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx| {
            gpui_kit::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(520.), px(160.)), cx)),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Status)).expect("open status window");
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.spawn(async move |cx| {
                while show_rx.recv().await.is_ok() {
                    let catalog = catalog.clone();
                    let _ = cx.update(move |cx| show(&catalog, cx));
                }
            })
            .detach();
        });
    }
}
