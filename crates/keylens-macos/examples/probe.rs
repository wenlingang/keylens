//! 开发用探测程序：在真机上验证 MacPlatform 的各项能力。
//! 打包成签名的 .app 后运行（见 CONTRIBUTING.md「本地运行需要辅助功能权限的程序」）。
//!
//!   probe trust
//!   probe read <pid>                  读取菜单，打印条目数、耗时和前 200 条
//!   probe press-verify <pid> <标题>    执行菜单项，比较执行前后的窗口列表
//!   probe tap <秒>                    打印按键事件
//!   probe tap-self                    模拟按 F19，确认监听收到
//!   probe tap-churn                   反复启动/停止按键监听，检查线程是否泄漏
//!   probe watch <秒>                  打印前台应用切换
//!   probe icon <pid> <输出.png>        保存应用图标

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
            for round in ["冷启动", "第二次"] {
                let started = Instant::now();
                let entries = platform.menu_source().load(&app(pid(1)));
                let elapsed = started.elapsed();
                match entries {
                    Ok(entries) => {
                        println!("[{round}] {} 条，耗时 {elapsed:?}", entries.len());
                        if round == "冷启动" {
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
                    Err(err) => println!("[{round}] 错误：{err}"),
                }
            }
        }
        Some("press-verify") => {
            let pid = pid(1);
            let title = &args[2];
            let entries = platform.menu_source().load(&app(pid)).expect("read menu");
            let Some(entry) = entries.iter().find(|e| &e.title == title) else {
                println!("FAIL：菜单里没有「{title}」");
                return;
            };
            let before = keylens_macos::debug::window_titles(pid);
            let result = platform.execute(entry);
            std::thread::sleep(Duration::from_millis(800));
            let after = keylens_macos::debug::window_titles(pid);
            println!("execute -> {result:?}");
            println!("窗口：{before:?} -> {after:?}");
            println!(
                "{}",
                if after != before {
                    "PASS：窗口有变化"
                } else {
                    "FAIL：窗口没有变化"
                }
            );
        }
        Some("tap") => {
            if platform.permission() == PermissionState::Denied {
                println!("没有辅助功能权限");
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
            // 启动监听后模拟按一次 F19（几乎没有应用使用），确认监听能收到事件。
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
            println!("收到 {} 个事件：{events:?}", events.len());
        }
        Some("tap-churn") => {
            // 反复启动并立即停止按键监听，检查监听线程是否都退出了。
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
            println!("50 次启动/停止后，进程线程数：{threads}");
        }
        Some("watch") => {
            // 诊断用：加上 `with-app` 参数会先初始化 NSApplication（gpui 应用里它总是存在）。
            if args.get(2).map(String::as_str) == Some("with-app") {
                let mtm = objc2::MainThreadMarker::new().expect("main thread");
                let _ = objc2_app_kit::NSApplication::sharedApplication(mtm);
                println!("已初始化 NSApplication");
            }
            println!("开始时前台：{:?}", platform.frontmost_app().map(|a| a.name));
            let (tx, rx) = channel();
            let _guard = platform.watch_frontmost(tx).expect("watch");
            let duration = secs(1);
            std::thread::spawn(move || {
                let poller = MacPlatform::default();
                let mut last = poller.frontmost_app().map(|a| a.pid);
                let deadline = Instant::now() + duration;
                while Instant::now() < deadline {
                    while let Ok(app) = rx.try_recv() {
                        println!("通知：{} pid={} ids={:?}", app.name, app.pid, app.ids);
                    }
                    let now = poller.frontmost_app();
                    if now.as_ref().map(|a| a.pid) != last {
                        println!("轮询发现：{:?}", now.as_ref().map(|a| &a.name));
                        last = now.map(|a| a.pid);
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                std::process::exit(0);
            });
            // 通知在主线程投递，需要运行主线程的 run loop。
            objc2_core_foundation::CFRunLoop::run();
        }
        Some("icon") => {
            let png = platform.app_icon(&app(pid(1)));
            match png {
                Some(bytes) => {
                    std::fs::write(&args[2], &bytes).expect("write png");
                    println!("已保存 {} 字节到 {}", bytes.len(), args[2]);
                }
                None => println!("没有拿到图标"),
            }
        }
        _ => eprintln!("用法见文件开头的注释"),
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("probe 只能在 macOS 上运行");
}
