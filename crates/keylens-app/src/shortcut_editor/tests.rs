// See view.rs: keep the built-in #[test] in scope next to gpui's.
use core::prelude::v1::test;

use gpui_kit::TestAppContext;
use keylens_core::Os;
use keylens_core::community::{AppData, parse_app_data};

use super::*;
use crate::recorder::SEQUENCE_WINDOW;

const FILE: &str = "com.keylens.test-editor.toml";

fn demo_app() -> AppInfo {
    AppInfo {
        pid: 7,
        name: "Demo".into(),
        ids: vec!["com.keylens.test-editor".into()],
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("keylens-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// An editor on a fresh temp file; returns the window and the file path.
fn editor(cx: &mut TestAppContext, name: &str) -> (WindowHandle<ShortcutEditor>, PathBuf) {
    cx.update(gpui_kit::init);
    let path = temp_dir(name).join(FILE);
    let file = path.clone();
    let window = cx.add_window(move |window, cx| {
        ShortcutEditor::new(demo_app(), file, Os::MacOS, |_| {}, || {}, window, cx)
    });
    // Focus events only reach an active window.
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    (window, path)
}

fn data(path: &Path) -> AppData {
    let text = std::fs::read_to_string(path).unwrap();
    parse_app_data(FILE, &text).unwrap_or_else(|e| panic!("{e:?}\n{text}"))
}

fn titles(data: &AppData) -> Vec<String> {
    data.groups
        .iter()
        .flat_map(|g| g.shortcuts.iter().map(|s| s.title.en.clone()))
        .collect()
}

/// Fills the open form and presses Save.
fn fill_and_save(
    view: &mut ShortcutEditor,
    title: &str,
    keys: &[&str],
    window: &mut Window,
    cx: &mut Context<ShortcutEditor>,
) {
    let form = view.form.as_mut().expect("a form is open");
    form.keys = keys.iter().map(|k| k.to_string()).collect();
    let input = form.title.clone();
    input.update(cx, |input, cx| {
        input.set_value(title.to_string(), window, cx)
    });
    view.save_form(window, cx);
}

fn add(window: WindowHandle<ShortcutEditor>, cx: &mut TestAppContext, title: &str, keys: &[&str]) {
    window
        .update(cx, |view, window, cx| {
            view.start_add(window, cx);
            fill_and_save(view, title, keys, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
}

fn read<R>(
    window: WindowHandle<ShortcutEditor>,
    cx: &mut TestAppContext,
    f: impl FnOnce(&ShortcutEditor) -> R,
) -> R {
    window.update(cx, |view, _, _| f(view)).unwrap()
}

#[gpui_kit::test]
fn adding_a_shortcut_writes_the_file_and_lists_it(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "add");
    assert!(!path.exists());
    add(window, cx, "Find", &["Mod+F"]);

    let data = data(&path);
    assert_eq!(data.groups.len(), 1);
    assert_eq!(data.groups[0].title.en, "General");
    assert_eq!(titles(&data), ["Find"]);
    assert_eq!(
        data.groups[0].shortcuts[0]
            .keys(Os::MacOS)
            .display(Os::MacOS),
        "⌘F"
    );
    read(window, cx, |view| {
        assert!(view.form.is_none());
        assert_eq!(view.problem, None);
        assert_eq!(view.groups[0].shortcuts[0].title, "Find");
        assert_eq!(view.groups[0].shortcuts[0].display, "⌘F");
    });
}

#[gpui_kit::test]
fn saving_checks_the_title_and_the_keys(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "checks");
    window
        .update(cx, |view, window, cx| {
            view.start_add(window, cx);
            fill_and_save(view, "   ", &["Mod+F"], window, cx);
            let error = view.form.as_ref().unwrap().error.clone();
            assert_eq!(error.as_deref(), Some("Enter a name."));
            fill_and_save(view, "Find", &[], window, cx);
            let error = view.form.as_ref().unwrap().error.clone();
            assert!(error.unwrap().contains("Mod+Shift+K"));
            view.choose_group(None, window, cx);
            fill_and_save(view, "Find", &["Mod+F"], window, cx);
            let error = view.form.as_ref().unwrap().error.clone();
            assert_eq!(error.as_deref(), Some("Enter a group name."));
        })
        .unwrap();
    assert!(!path.exists());
}

#[gpui_kit::test]
fn same_keys_block_saving(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "duplicate");
    add(window, cx, "Find", &["Mod+F"]);
    let before = std::fs::read(&path).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.start_add(window, cx);
            // Saving twice changes nothing: the data parser rejects two entries with the same keys.
            for _ in 0..2 {
                fill_and_save(view, "Search", &["Mod+F"], window, cx);
                let error = view.form.as_ref().expect("the form stays").error.clone();
                assert!(error.unwrap().contains("Find"));
            }
        })
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[gpui_kit::test]
fn editing_changes_the_entry_in_place(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "edit");
    add(window, cx, "Find", &["Mod+F"]);
    add(window, cx, "Save", &["Mod+S"]);
    window
        .update(cx, |view, window, cx| {
            view.start_edit(At { group: 0, index: 0 }, window, cx);
            assert_eq!(view.form.as_ref().unwrap().keys, ["Mod+F"]);
            fill_and_save(view, "Find in File", &["Mod+Shift+F"], window, cx);
        })
        .unwrap();
    let data = data(&path);
    assert_eq!(titles(&data), ["Find in File", "Save"]);
    assert_eq!(
        data.groups[0].shortcuts[0]
            .keys(Os::MacOS)
            .display(Os::MacOS),
        "⇧⌘F"
    );
}

#[gpui_kit::test]
fn enter_saves_and_escape_cancels(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "keys");
    window
        .update(cx, |view, window, cx| {
            view.start_add(window, cx);
            view.form.as_mut().unwrap().keys = vec!["Mod+F".into()];
        })
        .unwrap();
    cx.run_until_parked();
    cx.simulate_input(window.into(), "Find");
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(titles(&data(&path)), ["Find"]);
    assert!(read(window, cx, |view| view.form.is_none()));

    window
        .update(cx, |view, window, cx| view.start_add(window, cx))
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "escape");
    assert!(read(window, cx, |view| view.form.is_none()));
}

#[gpui_kit::test]
fn recording_turns_key_presses_into_chords(cx: &mut TestAppContext) {
    let (window, _) = editor(cx, "record");
    window
        .update(cx, |view, window, cx| view.start_add(window, cx))
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| window.focus(&view.recorder, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(read(window, cx, |view| view
        .form
        .as_ref()
        .unwrap()
        .recording));

    cx.simulate_keystrokes(window.into(), "cmd-shift-p");
    assert_eq!(
        read(window, cx, |view| view.form.as_ref().unwrap().keys.clone()),
        ["Mod+Shift+P"]
    );

    // A chord after the sequence window starts over; one inside it is the second step.
    cx.executor().advance_clock(SEQUENCE_WINDOW * 2);
    cx.simulate_keystrokes(window.into(), "cmd-k cmd-s");
    assert_eq!(
        read(window, cx, |view| view.form.as_ref().unwrap().keys.clone()),
        ["Mod+K", "Mod+S"]
    );

    cx.executor().advance_clock(SEQUENCE_WINDOW * 2);
    cx.simulate_keystrokes(window.into(), "cmd-p");
    assert_eq!(
        read(window, cx, |view| view.form.as_ref().unwrap().keys.clone()),
        ["Mod+P"]
    );

    // Backspace clears; Esc stops recording and puts back what was there when it started.
    cx.simulate_keystrokes(window.into(), "backspace");
    assert!(read(window, cx, |view| view
        .form
        .as_ref()
        .unwrap()
        .keys
        .is_empty()));
    cx.simulate_keystrokes(window.into(), "escape");
    read(window, cx, |view| {
        let form = view.form.as_ref().expect("Esc only leaves recording");
        assert!(!form.recording);
        assert!(form.keys.is_empty());
    });
}

#[gpui_kit::test]
fn an_invalid_file_is_read_only_and_untouched(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "invalid");
    add(window, cx, "Find", &["Mod+F"]);
    std::fs::write(&path, "schema = ").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.reload();
            assert!(matches!(view.problem, Some(EditorProblem::Invalid(_))));
            // The last good list stays on screen.
            assert_eq!(view.groups[0].shortcuts[0].title, "Find");
            view.start_add(window, cx);
            assert!(view.form.is_none());
            view.start_edit(At { group: 0, index: 0 }, window, cx);
            assert!(view.form.is_none());
            view.delete_at(At { group: 0, index: 0 }, window, cx);
        })
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "schema = ");

    // Fixed by hand: editable again.
    std::fs::write(&path, "schema = 1\n[app]\nname = \"Demo\"\n").unwrap();
    window
        .update(cx, |view, _, _| {
            view.reload();
            assert_eq!(view.problem, None);
            assert!(view.groups.is_empty());
        })
        .unwrap();
}

/// An editor whose data folder is a regular file: the user data file reads as missing, and
/// every write fails (for root too, unlike a read-only folder).
fn editor_on_a_blocked_folder(
    cx: &mut TestAppContext,
    name: &str,
) -> (WindowHandle<ShortcutEditor>, PathBuf) {
    let (window, path) = editor(cx, name);
    block_folder(&path);
    window.update(cx, |view, _, _| view.reload()).unwrap();
    (window, path)
}

/// Puts a regular file where the data folder of `path` goes.
fn block_folder(path: &Path) {
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    std::fs::write(dir, "not a folder").unwrap();
}

#[gpui_kit::test]
fn a_failed_write_keeps_the_form(cx: &mut TestAppContext) {
    let (window, path) = editor_on_a_blocked_folder(cx, "write");
    window
        .update(cx, |view, window, cx| {
            assert_eq!(view.problem, None, "a missing file is not an error");
            view.start_add(window, cx);
            fill_and_save(view, "Find", &["Mod+F"], window, cx);
            assert!(matches!(view.problem, Some(EditorProblem::WriteFailed(_))));
            assert!(view.form.is_some());
        })
        .unwrap();
    assert!(path.parent().unwrap().is_file());
}

#[gpui_kit::test]
fn delete_then_undo_restores_the_entry(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "undo");
    add(window, cx, "Find", &["Mod+F"]);
    add(window, cx, "Save", &["Mod+S"]);
    window
        .update(cx, |view, window, cx| {
            view.delete_at(At { group: 0, index: 0 }, window, cx)
        })
        .unwrap();
    assert_eq!(titles(&data(&path)), ["Save"]);
    read(window, cx, |view| {
        assert_eq!(view.undo.as_ref().unwrap().0.title, "Find");
        assert_eq!(view.groups[0].shortcuts.len(), 1);
    });

    window
        .update(cx, |view, window, cx| view.undo(window, cx))
        .unwrap();
    assert_eq!(titles(&data(&path)), ["Find", "Save"]);
    read(window, cx, |view| {
        assert!(view.undo.is_none());
        assert_eq!(view.groups[0].shortcuts[0].title, "Find");
    });

    // The undo bar goes away by itself.
    window
        .update(cx, |view, window, cx| {
            view.delete_at(At { group: 0, index: 1 }, window, cx)
        })
        .unwrap();
    assert!(read(window, cx, |view| view.undo.is_some()));
    cx.executor()
        .advance_clock(UNDO_FOR + Duration::from_millis(100));
    cx.run_until_parked();
    assert!(read(window, cx, |view| view.undo.is_none()));
}

#[gpui_kit::test]
fn opening_twice_keeps_one_window_and_switches_app(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("twice");
    let other = AppInfo {
        pid: 8,
        name: "Other".into(),
        ids: vec!["com.keylens.test-other".into()],
    };
    let first = cx.update(|cx| open_in(dir.clone(), demo_app(), Os::MacOS, |_| {}, || {}, cx));
    let (handle, view) = first.expect("a new window");
    handle
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.start_add(window, cx))
        })
        .unwrap();
    assert!(view.read_with(cx, |view, _| view.form.is_some()));
    let second = cx.update(|cx| open_in(dir.clone(), other, Os::MacOS, |_| {}, || {}, cx));
    assert!(second.is_none());
    assert_eq!(cx.windows().len(), 1);
    view.read_with(cx, |view, _| {
        assert_eq!(view.app.name, "Other");
        assert_eq!(view.path, dir.join("com.keylens.test-other.toml"));
        assert!(view.form.is_none());
    });
}

#[gpui_kit::test]
fn an_app_without_an_id_opens_nothing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let app = AppInfo {
        pid: 9,
        name: "NoId".into(),
        ids: vec![],
    };
    let opened = cx.update(|cx| open_in(temp_dir("noid"), app, Os::MacOS, |_| {}, || {}, cx));
    assert!(opened.is_none());
    assert_eq!(cx.windows().len(), 0);
}

#[gpui_kit::test]
fn the_window_title_is_kept_up_to_date(cx: &mut TestAppContext) {
    let (window, _) = editor(cx, "title");
    assert_eq!(
        read(window, cx, |view| view.shown_title.clone()),
        "Edit Shortcuts · Demo"
    );
    // Stands in for a language switch: a title that no longer matches is set again on the next
    // frame. (The test platform does not report window titles back.)
    window
        .update(cx, |view, _, cx| {
            view.shown_title = "stale".into();
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        read(window, cx, |view| view.shown_title.clone()),
        "Edit Shortcuts · Demo"
    );
}

#[gpui_kit::test]
fn the_close_shortcut_is_recorded_while_recording(cx: &mut TestAppContext) {
    // Through `open_in`, which binds the close shortcut.
    cx.update(gpui_kit::init);
    let opened = cx.update(|cx| {
        open_in(
            temp_dir("record-close"),
            demo_app(),
            // The keystroke below is the current platform's: ⌘W on macOS, Ctrl+W elsewhere.
            Os::current(),
            |_| {},
            || {},
            cx,
        )
    });
    let (handle, view) = opened.expect("a new window");
    handle
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.start_add(window, cx))
        })
        .unwrap();
    cx.run_until_parked();
    handle
        .update(cx, |_, window, cx| {
            let recorder = view.read(cx).recorder.clone();
            window.focus(&recorder, cx);
        })
        .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(
        handle,
        if cfg!(target_os = "macos") {
            "cmd-w"
        } else {
            "ctrl-w"
        },
    );
    cx.run_until_parked();
    assert_eq!(cx.windows().len(), 1);
    let keys = view.read_with(cx, |view, _| view.form.as_ref().unwrap().keys.clone());
    // The primary modifier is ⌘ on macOS and Ctrl elsewhere: both are "Mod".
    assert_eq!(keys, ["Mod+W"]);
}

#[gpui_kit::test]
fn an_entry_gone_from_the_file_closes_its_form(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "stale");
    add(window, cx, "Find", &["Mod+F"]);
    add(window, cx, "Save", &["Mod+S"]);
    add(window, cx, "Open", &["Mod+O"]);
    window
        .update(cx, |view, window, cx| {
            view.start_edit(At { group: 0, index: 0 }, window, cx)
        })
        .unwrap();
    // Edited by hand: "Find" removed, so position 0 now holds "Save".
    let text = std::fs::read_to_string(&path).unwrap();
    let text = text.replace(
        "[[group.shortcut]]\ntitle = \"Find\"\nkeys = \"Mod+F\"\n\n",
        "",
    );
    std::fs::write(&path, &text).unwrap();
    window
        .update(cx, |view, _, cx| {
            view.reload();
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    read(window, cx, |view| {
        assert_eq!(
            view.shortcut_title(At { group: 0, index: 0 }).as_deref(),
            Some("Save")
        );
        assert!(view.form.is_none());
        assert_eq!(view.problem, Some(EditorProblem::Stale));
    });
    // "Add" works again.
    window
        .update(cx, |view, window, cx| {
            view.start_add(window, cx);
            assert!(view.form.is_some());
        })
        .unwrap();
}

#[gpui_kit::test]
fn an_entry_that_moved_keeps_its_form(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "moved");
    add(window, cx, "Find", &["Mod+F"]);
    add(window, cx, "Save", &["Mod+S"]);
    window
        .update(cx, |view, window, cx| {
            view.start_edit(At { group: 0, index: 1 }, window, cx)
        })
        .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let text = text.replace(
        "[[group.shortcut]]\ntitle = \"Find\"\nkeys = \"Mod+F\"\n\n",
        "",
    );
    std::fs::write(&path, &text).unwrap();
    window
        .update(cx, |view, window, cx| {
            view.reload();
            let form = view.form.as_ref().expect("still open");
            assert_eq!(form.target, Some(At { group: 0, index: 0 }));
            fill_and_save(view, "Save All", &["Mod+Alt+S"], window, cx);
        })
        .unwrap();
    assert_eq!(titles(&data(&path)), ["Save All"]);
}

#[gpui_kit::test]
fn deleting_leaves_an_open_form_alone(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "delete-form");
    add(window, cx, "Find", &["Mod+F"]);
    window
        .update(cx, |view, window, cx| {
            view.start_add(window, cx);
            let input = view.form.as_ref().unwrap().title.clone();
            input.update(cx, |input, cx| input.set_value("Half typed", window, cx));
            view.delete_at(At { group: 0, index: 0 }, window, cx);
            let form = view.form.as_ref().expect("the form stays open");
            assert_eq!(form.title.read(cx).value().to_string(), "Half typed");
            assert_eq!(form.error, None);
        })
        .unwrap();
    assert!(titles(&data(&path)).is_empty());
}

#[gpui_kit::test]
fn a_failed_undo_keeps_the_undo_bar(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "undo-fail");
    add(window, cx, "Find", &["Mod+F"]);
    add(window, cx, "Save", &["Mod+S"]);
    window
        .update(cx, |view, window, cx| {
            view.delete_at(At { group: 0, index: 0 }, window, cx)
        })
        .unwrap();
    // Swap the data folder for a file, so restoring cannot be written.
    let dir = path.parent().unwrap();
    let aside = dir.with_extension("aside");
    let _ = std::fs::remove_dir_all(&aside);
    std::fs::rename(dir, &aside).unwrap();
    std::fs::write(dir, "not a folder").unwrap();
    window
        .update(cx, |view, window, cx| {
            view.undo(window, cx);
            assert!(view.undo.is_some());
            assert!(matches!(view.problem, Some(EditorProblem::Failed(_))));
        })
        .unwrap();
    std::fs::remove_file(dir).unwrap();
    std::fs::rename(&aside, dir).unwrap();
    window
        .update(cx, |view, window, cx| view.undo(window, cx))
        .unwrap();
    assert_eq!(titles(&data(&path)), ["Find", "Save"]);
}

#[gpui_kit::test]
fn a_file_with_only_comments_gets_the_template(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "comments");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "# notes\n").unwrap();
    add(window, cx, "Find", &["Mod+F"]);
    assert_eq!(titles(&data(&path)), ["Find"]);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("# notes\n")
    );
}

static BEEPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[gpui_kit::test]
fn open_in_editor_beeps_when_the_file_cannot_be_created(cx: &mut TestAppContext) {
    use std::sync::atomic::Ordering;

    cx.update(gpui_kit::init);
    let path = temp_dir("open-fail").join(FILE);
    block_folder(&path);
    let window = cx.add_window(move |window, cx| {
        ShortcutEditor::new(
            demo_app(),
            path,
            Os::MacOS,
            |_| panic!("nothing to open"),
            || {
                BEEPS.fetch_add(1, Ordering::SeqCst);
            },
            window,
            cx,
        )
    });
    window
        .update(cx, |view, _, _| view.open_in_editor())
        .unwrap();
    assert_eq!(BEEPS.load(Ordering::SeqCst), 1);
}

#[gpui_kit::test]
fn an_edit_the_data_parser_would_reject_is_not_written(cx: &mut TestAppContext) {
    let (window, path) = editor(cx, "loadable");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Per-platform keys are skipped by the friendly duplicate check, but on macOS this entry
    // uses Mod+F, so the data parser would reject a second Mod+F.
    let text = "schema = 1\n\n[app]\nname = \"Demo\"\n\n[app.match]\n\
                macos = [\"com.keylens.test-editor\"]\nlinux = [\"demo\"]\n\n\
                [[group]]\ntitle = \"General\"\n\n[[group.shortcut]]\ntitle = \"Find\"\n\
                keys = { default = \"Mod+F\", linux = \"Ctrl+G\" }\n";
    std::fs::write(&path, text).unwrap();
    data(&path);
    window
        .update(cx, |view, window, cx| {
            view.reload();
            view.start_add(window, cx);
            fill_and_save(view, "Search", &["Mod+F"], window, cx);
            let form = view.form.as_ref().expect("the form stays");
            assert!(form.error.as_ref().unwrap().contains("already used"));
        })
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[gpui_kit::test]
fn set_app_for_the_same_app_keeps_the_open_form(cx: &mut TestAppContext) {
    let (window, _) = editor(cx, "same-app");
    window
        .update(cx, |view, window, cx| {
            view.start_add(window, cx);
            let input = view.form.as_ref().unwrap().title.clone();
            input.update(cx, |input, cx| {
                input.set_value("Typed".to_string(), window, cx)
            });
            view.set_app(demo_app(), window, cx);
            let form = view.form.as_ref().expect("the form stays");
            assert_eq!(form.title.read(cx).value().to_string(), "Typed");
        })
        .unwrap();
}

#[gpui_kit::test]
fn the_editor_edits_a_hand_named_file_for_the_app(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("hand-named");
    std::fs::create_dir_all(&dir).unwrap();
    let hand = dir.join("mine.toml");
    std::fs::write(
        &hand,
        "schema = 1\n\n[app]\nname = \"Demo\"\n\n[app.match]\n\
         macos = [\"com.keylens.test-editor\"]\n",
    )
    .unwrap();
    let (_, view) = cx
        .update(|cx| open_in(dir.clone(), demo_app(), Os::MacOS, |_| {}, || {}, cx))
        .expect("a new window");
    view.read_with(cx, |view, _| assert_eq!(view.path, hand));
}
