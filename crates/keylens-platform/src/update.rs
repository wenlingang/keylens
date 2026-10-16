//! What the in-app updater needs from the platform: replacing the running app with a downloaded
//! one, relaunching it, and showing a dialog. Checking and downloading are shared (keylens-app).

use std::path::{Path, PathBuf};

/// Why a downloaded update could not be installed.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// Keylens is not running from an app bundle (e.g. `cargo run`).
    #[error("not running from an app bundle")]
    NotInApp,
    /// macOS runs the app from a read-only random path (App Translocation) because it was opened
    /// where it was downloaded instead of being moved to Applications.
    #[error("the app is translocated: {0}")]
    Translocated(PathBuf),
    /// The running app's signature does not pin a certificate (ad-hoc or unsigned), so there is
    /// nothing to check the update against.
    #[error("the running app is not signed with a certificate: {0}")]
    Unsigned(String),
    #[error("could not read the update package: {0}")]
    Package(String),
    /// The update is not signed by the same certificate as the running app.
    #[error("the update's signature does not match: {0}")]
    Signature(String),
    #[error("could not replace {path}: {detail}")]
    Replace { path: PathBuf, detail: String },
}

/// A dialog with a title, a short message, optional details and buttons. The first button is the
/// default (Return).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub title: String,
    pub message: String,
    /// Longer text (release notes) shown in a fixed-size scrolling area below the message.
    pub details: Option<String>,
    pub buttons: Vec<String>,
    /// The button Escape presses.
    pub cancel: Option<usize>,
}

/// Platform functions for the updater.
#[derive(Clone, Copy)]
pub struct UpdateNative {
    /// Installs the app inside the disk image `package` over the app bundle `app`, after checking
    /// that it is signed by the same certificate as `app` and is `version` (so an old, validly
    /// signed build cannot be passed off as a newer one). Blocking; call it off the main thread.
    pub install: fn(package: &Path, app: &Path, version: &str) -> Result<(), InstallError>,
    /// Starts `app` again once this process has exited. The caller quits right after.
    pub relaunch: fn(app: &Path) -> std::io::Result<()>,
    /// Shows a non-blocking dialog on the main thread; `done` gets the index of the button clicked.
    pub dialog: fn(Dialog, Box<dyn FnOnce(usize)>),
}

/// The `.app` bundle that contains the executable `exe` (`…/Keylens.app/Contents/MacOS/Keylens`).
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is_app = app.extension().is_some_and(|e| e == "app");
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app)
        .then(|| app.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_of_finds_the_app_around_its_executable() {
        assert_eq!(
            bundle_of(Path::new(
                "/Applications/Keylens.app/Contents/MacOS/Keylens"
            )),
            Some(PathBuf::from("/Applications/Keylens.app"))
        );
    }

    #[test]
    fn bundle_of_rejects_a_bare_executable() {
        assert_eq!(bundle_of(Path::new("/repo/target/debug/keylens")), None);
        assert_eq!(bundle_of(Path::new("/x/Contents/MacOS/Keylens")), None);
        assert_eq!(bundle_of(Path::new("Keylens")), None);
    }
}
