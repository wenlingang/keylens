//! Installing an update: mount the downloaded .dmg, copy the app next to the running one, check its
//! signature against the running app's designated requirement, swap the bundles, relaunch.
//! Also the update dialog, an NSAlert shown without a modal session so gpui keeps running.

use std::cell::RefCell;
use std::path::Path;
use std::process::{Command, Output};

use keylens_platform::update::{Dialog, InstallError};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSAlert, NSApplication, NSButton};
use objc2_foundation::NSString;

const APP_NAME: &str = "Keylens.app";
/// Bundles the installer puts next to the app while it works (`<prefix>update-<pid>.app`, …).
const LEFTOVER_PREFIX: &str = ".Keylens-";

/// Deletes bundles left by an install that was killed half-way.
fn remove_leftovers(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(LEFTOVER_PREFIX) && name.ends_with(".app") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

pub fn install(package: &Path, app: &Path, version: &str) -> Result<(), InstallError> {
    if app
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Err(InstallError::Translocated(app.to_path_buf()));
    }
    let requirement = designated_requirement(app)?;
    let parent = app
        .parent()
        .ok_or_else(|| InstallError::Package(format!("{} has no parent folder", app.display())))?;
    remove_leftovers(parent);
    let pid = std::process::id();
    let staged = parent.join(format!("{LEFTOVER_PREFIX}update-{pid}.app"));
    let old = parent.join(format!("{LEFTOVER_PREFIX}old-{pid}.app"));

    let result = copy_out_of(package, &staged)
        .and_then(|()| verify(&staged, &requirement))
        .and_then(|()| check_version(&staged, version))
        .and_then(|()| swap(app, &staged, &old));
    let _ = std::fs::remove_dir_all(&staged);
    result
}

/// The running app's designated requirement, e.g.
/// `identifier "io.github.wenlingang.keylens" and certificate leaf = H"…"`.
fn designated_requirement(app: &Path) -> Result<String, InstallError> {
    let out = run(Command::new("/usr/bin/codesign")
        .arg("-d")
        .arg("-r-")
        .arg(app))
    .map_err(InstallError::Unsigned)?;
    // codesign prints the requirement on stdout, other details on stderr.
    let text = String::from_utf8_lossy(&out.stdout);
    parse_requirement(&text).ok_or_else(|| InstallError::Unsigned(text.trim().to_string()))
}

/// Takes the `designated => …` line; refuses requirements that do not pin a certificate (ad-hoc
/// signatures only pin a cdhash, which every new build changes).
fn parse_requirement(text: &str) -> Option<String> {
    let requirement = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("designated => "))?;
    requirement
        .contains("certificate ")
        .then(|| requirement.to_string())
}

fn copy_out_of(package: &Path, staged: &Path) -> Result<(), InstallError> {
    let mount = std::env::temp_dir().join(format!("keylens-mount-{}", std::process::id()));
    std::fs::create_dir_all(&mount).map_err(|e| InstallError::Package(e.to_string()))?;
    let copied = run(Command::new("/usr/bin/hdiutil")
        .args([
            "attach",
            "-quiet",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(package))
    .and_then(|_| {
        let copied = run(Command::new("/usr/bin/ditto")
            .arg(mount.join(APP_NAME))
            .arg(staged));
        let _ = run(Command::new("/usr/bin/hdiutil")
            .args(["detach", "-quiet", "-force"])
            .arg(&mount));
        copied
    });
    let _ = std::fs::remove_dir(&mount);
    copied.map(|_| ()).map_err(InstallError::Package)
}

fn verify(staged: &Path, requirement: &str) -> Result<(), InstallError> {
    run(Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(format!("-R={requirement}"))
        .arg(staged))
    .map(|_| ())
    .map_err(InstallError::Signature)
}

fn check_version(staged: &Path, expected: &str) -> Result<(), InstallError> {
    let out = run(Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleShortVersionString"])
        .arg(staged.join("Contents/Info.plist")))
    .map_err(InstallError::Package)?;
    let found = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if found == expected {
        Ok(())
    } else {
        Err(InstallError::Package(format!(
            "the package contains version {found}, expected {expected}"
        )))
    }
}

/// Moves the running app aside, moves the new one into its place, then deletes the old one. The
/// running process keeps working: its files stay open after the rename.
fn swap(app: &Path, staged: &Path, old: &Path) -> Result<(), InstallError> {
    let replace_error = |e: std::io::Error| InstallError::Replace {
        path: app.to_path_buf(),
        detail: e.to_string(),
    };
    std::fs::rename(app, old).map_err(replace_error)?;
    if let Err(e) = std::fs::rename(staged, app) {
        let _ = std::fs::rename(old, app);
        return Err(replace_error(e));
    }
    let _ = std::fs::remove_dir_all(old);
    Ok(())
}

/// Runs a command; on failure returns its stderr (or the spawn error).
fn run(command: &mut Command) -> Result<Output, String> {
    let out = command.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            out.status.to_string()
        } else {
            stderr
        })
    }
}

/// Opens `app` once this process has exited (`open` would otherwise just activate us).
pub fn relaunch(app: &Path) -> std::io::Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while kill -0 "$0" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$1""#)
        .arg(std::process::id().to_string())
        .arg(app)
        .spawn()
        .map(|_| ())
}

// ---- Dialog ----

/// Called with the index of the clicked button.
type OnClick = Box<dyn FnOnce(usize)>;

pub struct DialogIvars {
    alert: RefCell<Option<Retained<NSAlert>>>,
    done: RefCell<Option<OnClick>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeylensDialogTarget"]
    #[ivars = DialogIvars]
    struct DialogTarget;

    impl DialogTarget {
        #[unsafe(method(buttonClicked:))]
        fn button_clicked(&self, sender: &NSButton) {
            // Read the button first: it belongs to the alert released below.
            let index = sender.tag() as usize;
            let alert = self.ivars().alert.borrow_mut().take();
            if let Some(alert) = &alert {
                alert.window().orderOut(None);
            }
            let done = self.ivars().done.borrow_mut().take();
            if let Some(done) = done {
                done(index);
            }
            drop(alert);
            // Releases this target: nothing may touch `self` after this line.
            OPEN.with(|open| open.borrow_mut().retain(|t| !std::ptr::eq(&**t, self)));
        }
    }
);

thread_local! {
    /// Dialogs on screen; keeps their targets (weakly referenced by the buttons) alive.
    static OPEN: RefCell<Vec<Retained<DialogTarget>>> = const { RefCell::new(Vec::new()) };
}

/// Shows `dialog` in front of other apps and returns at once; `done` runs on the main thread with
/// the clicked button's index. No modal session: it would stall gpui's main-thread tasks.
pub fn dialog(dialog: Dialog, done: Box<dyn FnOnce(usize)>) {
    let mtm = MainThreadMarker::new().expect("dialogs must be shown on the main thread");
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&dialog.title));
    alert.setInformativeText(&NSString::from_str(&dialog.message));
    let target = DialogTarget::alloc(mtm).set_ivars(DialogIvars {
        alert: RefCell::new(None),
        done: RefCell::new(Some(done)),
    });
    // SAFETY: Calls NSObject's init.
    let target: Retained<DialogTarget> = unsafe { msg_send![super(target), init] };
    let last = dialog.buttons.len().saturating_sub(1);
    for (index, title) in dialog.buttons.iter().enumerate() {
        let button = alert.addButtonWithTitle(&NSString::from_str(title));
        button.setTag(index as isize);
        // SAFETY: `target` stays alive in OPEN until a button is clicked.
        unsafe {
            button.setTarget(Some(&*target as &AnyObject));
            button.setAction(Some(sel!(buttonClicked:)));
        }
        if index == last && index > 0 {
            button.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        }
    }
    *target.ivars().alert.borrow_mut() = Some(alert.clone());
    OPEN.with(|open| open.borrow_mut().push(target));

    alert.layout();
    let window = alert.window();
    window.center();
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_requirement_takes_the_designated_line() {
        let text = "# designated => identifier \"x\"\ndesignated => identifier \"io.github.wenlingang.keylens\" and certificate leaf = H\"ABC\"\n";
        assert_eq!(
            parse_requirement(text).as_deref(),
            Some(r#"identifier "io.github.wenlingang.keylens" and certificate leaf = H"ABC""#)
        );
    }

    #[test]
    fn parse_requirement_refuses_ad_hoc_signatures() {
        assert_eq!(parse_requirement("designated => cdhash H\"0123\"\n"), None);
        assert_eq!(
            parse_requirement("code object is not signed at all\n"),
            None
        );
    }
}
