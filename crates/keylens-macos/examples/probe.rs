//! Dev probe: verifies MacPlatform's capabilities on a real machine.
//! Run it after bundling into a signed .app (see CONTRIBUTING.md, "Running programs that need Accessibility permission locally").
//!
//!   probe trust
//!   probe read <pid>                  read the menu; print entry count, elapsed time and the first 200 entries
//!   probe press-verify <pid> <title>  execute a menu item and compare the window list before and after
//!   probe tap <secs>                  print key events
//!   probe tap-self                    synthesize an F19 press to confirm the listener receives it
//!   probe tap-churn                   repeatedly start/stop the key listener to check for thread leaks
//!   probe watch <secs>                print frontmost app changes
//!   probe icon <pid> <out.png>        save the app icon

#[cfg(target_os = "macos")]
fn main() {
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    use keylens_core::{AppInfo, Os};
    use keylens_macos::MacPlatform;
    use keylens_platform::{PermissionState, Platform};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let platform = MacPlatform::default();
    let pid = |i: usize| -> i32 { args[i].parse().expect("pid") };
    let app = |pid: i32| AppInfo {
        pid,
        name: String::new(),
        ids: vec![],
    };
    let secs =
        |i: usize| Duration::from_secs(args.get(i).and_then(|s| s.parse().ok()).unwrap_or(15));

    match args.first().map(String::as_str) {
        Some("trust") => println!("permission = {:?}", platform.permission()),
        Some("read") => {
            for round in ["cold", "warm"] {
                let started = Instant::now();
                let entries = platform.menu_source().load(&app(pid(1)));
                let elapsed = started.elapsed();
                match entries {
                    Ok(entries) => {
                        println!("[{round}] {} entries, took {elapsed:?}", entries.len());
                        if round == "cold" {
                            for e in entries.iter().take(200) {
                                let path =
                                    [vec![e.group.clone()], e.path.clone(), vec![e.title.clone()]]
                                        .concat()
                                        .join(" › ");
                                println!(
                                    "  {path}  {}  enabled={}",
                                    e.shortcut.display(Os::MacOS),
                                    e.enabled
                                );
                            }
                        }
                    }
                    Err(err) => println!("[{round}] error: {err}"),
                }
            }
        }
        Some("press-verify") => {
            let pid = pid(1);
            let title = &args[2];
            let entries = platform.menu_source().load(&app(pid)).expect("read menu");
            let Some(entry) = entries.iter().find(|e| &e.title == title) else {
                println!("FAIL: no \"{title}\" in the menu");
                return;
            };
            let before = keylens_macos::debug::window_titles(pid);
            let result = platform.execute(entry);
            std::thread::sleep(Duration::from_millis(800));
            let after = keylens_macos::debug::window_titles(pid);
            println!("execute -> {result:?}");
            println!("window: {before:?} -> {after:?}");
            println!(
                "{}",
                if after != before {
                    "PASS: window changed"
                } else {
                    "FAIL: window did not change"
                }
            );
        }
        Some("tap") => {
            if platform.permission() == PermissionState::Denied {
                println!("no Accessibility permission");
                return;
            }
            let (tx, rx) = channel();
            let _guard = platform.start_key_events(tx).expect("start key events");
            let deadline = Instant::now() + secs(1);
            while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                if let Ok(event) = rx.recv_timeout(left) {
                    println!("{event:?}");
                }
            }
        }
        Some("tap-self") => {
            // After starting the listener, synthesize one F19 press (almost no app uses it) to confirm the listener receives events.
            let (tx, rx) = channel();
            let _guard = platform.start_key_events(tx).expect("start key events");
            let f19 = keylens_core::Entry {
                id: keylens_core::EntryId {
                    source: keylens_core::EntrySource::Community,
                    snapshot: 0,
                    index: 0,
                },
                group: String::new(),
                path: vec![],
                title: "F19".into(),
                search_terms: vec![],
                shortcut: keylens_core::Shortcut::parse_seq(&["F19"], Os::MacOS).unwrap(),
                enabled: true,
            };
            println!("execute -> {:?}", platform.execute(&f19));
            std::thread::sleep(Duration::from_millis(500));
            let events: Vec<_> = rx.try_iter().collect();
            println!("received {} events: {events:?}", events.len());
        }
        Some("tap-churn") => {
            // Repeatedly start and immediately stop the key listener, then check that all listener threads exited.
            for _ in 0..50 {
                let (tx, _rx) = channel();
                drop(platform.start_key_events(tx).expect("start key events"));
            }
            std::thread::sleep(Duration::from_secs(2));
            let output = std::process::Command::new("ps")
                .args(["-M", "-p", &std::process::id().to_string()])
                .output()
                .expect("ps");
            let threads = String::from_utf8_lossy(&output.stdout)
                .lines()
                .count()
                .saturating_sub(1);
            println!("thread count after 50 start/stop cycles: {threads}");
        }
        Some("watch") => {
            // Diagnostic: the `with-app` argument initializes NSApplication first (it always exists in a gpui app).
            if args.get(2).map(String::as_str) == Some("with-app") {
                let mtm = objc2::MainThreadMarker::new().expect("main thread");
                let _ = objc2_app_kit::NSApplication::sharedApplication(mtm);
                println!("NSApplication initialized");
            }
            println!(
                "frontmost at start: {:?}",
                platform.frontmost_app().map(|a| a.name)
            );
            let (tx, rx) = channel();
            let _guard = platform.watch_frontmost(tx).expect("watch");
            let duration = secs(1);
            std::thread::spawn(move || {
                let poller = MacPlatform::default();
                let mut last = poller.frontmost_app().map(|a| a.pid);
                let deadline = Instant::now() + duration;
                while Instant::now() < deadline {
                    while let Ok(app) = rx.try_recv() {
                        println!(
                            "notification: {} pid={} ids={:?}",
                            app.name, app.pid, app.ids
                        );
                    }
                    let now = poller.frontmost_app();
                    if now.as_ref().map(|a| a.pid) != last {
                        println!("found by polling: {:?}", now.as_ref().map(|a| &a.name));
                        last = now.map(|a| a.pid);
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                std::process::exit(0);
            });
            // Notifications are delivered on the main thread, so the main thread's run loop must run.
            objc2_core_foundation::CFRunLoop::run();
        }
        Some("icon") => {
            let png = platform.app_icon(&app(pid(1)));
            match png {
                Some(bytes) => {
                    std::fs::write(&args[2], &bytes).expect("write png");
                    println!("saved {} bytes to {}", bytes.len(), args[2]);
                }
                None => println!("no icon returned"),
            }
        }
        _ => eprintln!("usage: see the comment at the top of the file"),
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("probe only runs on macOS");
}
