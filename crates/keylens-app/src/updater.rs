//! In-app updates: checks for a new release on a timer and from the menu bar, asks before
//! installing, downloads and installs it, then relaunches. The steps live in `update` (check and
//! download) and the platform's `UpdateNative` (install, relaunch, dialogs).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AsyncApp, Context, Task, WeakEntity};
use keylens_platform::update::{Dialog, InstallError, UpdateNative, bundle_of};
use rust_i18n::t;

use crate::update::{self, Http, Release, UpdateError};

/// Delay before the first automatic check, so it does not compete with startup.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Release notes longer than this are cut in the dialog.
const NOTES_LIMIT: usize = 600;

/// Why an update failed, for the error dialog.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error(transparent)]
    Update(#[from] UpdateError),
    #[error(transparent)]
    Install(#[from] InstallError),
}

pub struct Updater {
    http: Arc<dyn Http>,
    native: UpdateNative,
    /// Where the running app is installed; `None` when not running from an app bundle.
    app: Option<PathBuf>,
    /// Folder for the downloaded .dmg.
    download_dir: PathBuf,
    /// Told when a check or install starts (`true`) and ends (`false`).
    on_busy: Box<dyn Fn(bool)>,
    busy: bool,
    schedule: Option<Task<()>>,
    job: Option<Task<()>>,
}

impl Updater {
    pub fn new(http: Arc<dyn Http>, native: UpdateNative, on_busy: Box<dyn Fn(bool)>) -> Self {
        Self {
            http,
            native,
            app: std::env::current_exe().ok().and_then(|exe| bundle_of(&exe)),
            download_dir: std::env::temp_dir().join("keylens-update"),
            on_busy,
            busy: false,
            schedule: None,
            job: None,
        }
    }

    /// Checks shortly after startup and then once a day, telling the user only about new versions.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.schedule = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FIRST_CHECK).await;
            loop {
                if this.update(cx, |this, cx| this.check(false, cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(CHECK_INTERVAL).await;
            }
        }));
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Checks for a new version. `manual` (from the menu) also reports "up to date" and errors;
    /// automatic checks only log them. Stays busy until the last dialog is answered, so a second
    /// check cannot replace (and cancel) a job that is waiting for the user.
    pub fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.set_busy(true);
        let http = self.http.clone();
        self.job = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { update::check(&*http, CURRENT) })
                .await;
            match result {
                Ok(Some(release)) => {
                    tracing::info!(version = %release.version, "update available");
                    let install = ask(&this, available_dialog(&release), cx).await == Some(0);
                    if install {
                        // Runs on as this job: `install` must not replace `self.job` from inside it.
                        install_release(&this, release, cx).await;
                        return;
                    }
                }
                Ok(None) => {
                    tracing::info!("no update available");
                    if manual {
                        ask(&this, up_to_date_dialog(), cx).await;
                    }
                }
                Err(e) => {
                    tracing::warn!("update check failed: {e}");
                    if manual {
                        ask(
                            &this,
                            failed_dialog(t!("Update.check_failed"), &e.into()),
                            cx,
                        )
                        .await;
                    }
                }
            }
            let _ = this.update(cx, |this, _| this.set_busy(false));
        }));
    }

    fn set_busy(&mut self, busy: bool) {
        if self.busy != busy {
            self.busy = busy;
            (self.on_busy)(busy);
        }
    }
}

/// Downloads and installs `release`, then relaunches (quitting this process); on failure tells the
/// user why and ends the busy state.
async fn install_release(this: &WeakEntity<Updater>, release: Release, cx: &mut AsyncApp) {
    let Ok((http, native, app, dir)) = this.update(cx, |this, _| {
        (
            this.http.clone(),
            this.native,
            this.app.clone(),
            this.download_dir.clone(),
        )
    }) else {
        return;
    };
    let result = cx
        .background_executor()
        .spawn(async move {
            let app = app.ok_or(InstallError::NotInApp)?;
            let package = update::download(&*http, &release, &dir)?;
            let installed = (native.install)(&package, &app, &release.version.to_string());
            let _ = std::fs::remove_dir_all(&dir);
            installed?;
            Ok::<_, Failure>(app)
        })
        .await;
    match result {
        Ok(app) => {
            tracing::info!("update installed; relaunching");
            match (native.relaunch)(&app) {
                Ok(()) => return cx.update(|cx| cx.quit()),
                Err(e) => {
                    tracing::warn!("relaunch failed: {e}");
                    ask(this, restart_dialog(), cx).await;
                }
            }
        }
        Err(e) => {
            tracing::warn!("update failed: {e}");
            ask(this, failed_dialog(t!("Update.install_failed"), &e), cx).await;
        }
    }
    let _ = this.update(cx, |this, _| this.set_busy(false));
}

/// Shows `dialog` and waits for the clicked button; `None` if the updater is gone.
async fn ask(this: &WeakEntity<Updater>, dialog: Dialog, cx: &mut AsyncApp) -> Option<usize> {
    let (tx, rx) = async_channel::bounded(1);
    this.update(cx, |this, _| {
        (this.native.dialog)(
            dialog,
            Box::new(move |button| {
                let _ = tx.try_send(button);
            }),
        )
    })
    .ok()?;
    rx.recv().await.ok()
}

fn available_dialog(release: &Release) -> Dialog {
    let mut message = t!("Update.available_body", current = CURRENT).to_string();
    if !release.notes.is_empty() {
        message.push_str("\n\n");
        message.push_str(&truncate(&release.notes, NOTES_LIMIT));
    }
    Dialog {
        title: t!("Update.available_title", version = release.version).to_string(),
        message,
        buttons: vec![
            t!("Update.install").to_string(),
            t!("Update.later").to_string(),
        ],
    }
}

fn up_to_date_dialog() -> Dialog {
    Dialog {
        title: t!("Update.up_to_date_title").to_string(),
        message: t!("Update.up_to_date_body", version = CURRENT).to_string(),
        buttons: vec![t!("Update.ok").to_string()],
    }
}

fn restart_dialog() -> Dialog {
    Dialog {
        title: t!("Update.installed_title").to_string(),
        message: t!("Update.installed_body").to_string(),
        buttons: vec![t!("Update.ok").to_string()],
    }
}

fn failed_dialog(title: impl Into<String>, failure: &Failure) -> Dialog {
    Dialog {
        title: title.into(),
        message: failure_message(failure),
        buttons: vec![t!("Update.ok").to_string()],
    }
}

/// What went wrong, in the UI language, with the technical detail where it helps.
pub fn failure_message(failure: &Failure) -> String {
    match failure {
        Failure::Update(UpdateError::Network(detail)) => {
            t!("Update.err_network", detail = detail).to_string()
        }
        Failure::Update(UpdateError::Manifest(_)) => t!("Update.err_manifest").to_string(),
        Failure::Update(UpdateError::Checksum { .. }) => t!("Update.err_checksum").to_string(),
        Failure::Update(UpdateError::Io(e)) => t!("Update.err_other", detail = e).to_string(),
        Failure::Install(InstallError::NotInApp) => t!("Update.err_not_in_app").to_string(),
        Failure::Install(InstallError::Translocated(_)) => {
            t!("Update.err_translocated").to_string()
        }
        Failure::Install(InstallError::Unsigned(_)) => t!("Update.err_unsigned").to_string(),
        Failure::Install(InstallError::Signature(_)) => t!("Update.err_signature").to_string(),
        Failure::Install(InstallError::Package(detail)) => {
            t!("Update.err_other", detail = detail).to_string()
        }
        Failure::Install(InstallError::Replace { path, detail }) => {
            t!("Update.err_replace", path = path.display(), detail = detail).to_string()
        }
    }
}

fn truncate(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    // See view.rs: keep the built-in #[test] in scope next to gpui's.
    use core::prelude::v1::test;

    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gpui_kit::{AppContext, Entity, TestAppContext};

    use super::*;

    const SHA_OF_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    struct FakeHttp {
        manifest: Result<String, String>,
        downloads: Mutex<usize>,
    }

    impl Http for FakeHttp {
        fn get(&self, _: &str) -> Result<Vec<u8>, String> {
            self.manifest.clone().map(String::into_bytes)
        }
        fn download(&self, _: &str, to: &Path) -> Result<(), String> {
            *self.downloads.lock().unwrap() += 1;
            std::fs::write(to, b"").map_err(|e| e.to_string())
        }
    }

    fn release_manifest(version: &str) -> Result<String, String> {
        Ok(format!(
            r#"{{"version":"{version}","dmg":"Keylens-{version}.dmg","sha256":"{SHA_OF_EMPTY}","notes":"New things"}}"#
        ))
    }

    thread_local! {
        /// Dialogs shown, and the button the fake user clicks on each.
        static DIALOGS: RefCell<Vec<Dialog>> = const { RefCell::new(Vec::new()) };
        /// `None`: the fake user leaves the dialog open (its callback goes to PENDING).
        static CLICK: RefCell<Option<usize>> = const { RefCell::new(Some(0)) };
        static PENDING: RefCell<Option<OnClick>> = const { RefCell::new(None) };
        static INSTALLS: RefCell<Vec<(PathBuf, String)>> = const { RefCell::new(Vec::new()) };
        static RELAUNCHES: RefCell<usize> = const { RefCell::new(0) };
    }

    fn native(install: InstallFn) -> UpdateNative {
        UpdateNative {
            install,
            relaunch: |_| {
                RELAUNCHES.with(|r| *r.borrow_mut() += 1);
                Ok(())
            },
            dialog: |dialog, done| {
                DIALOGS.with(|d| d.borrow_mut().push(dialog));
                match CLICK.with(|c| *c.borrow()) {
                    Some(button) => done(button),
                    None => PENDING.with(|p| *p.borrow_mut() = Some(done)),
                }
            },
        }
    }

    type InstallFn = fn(&Path, &Path, &str) -> Result<(), InstallError>;
    type OnClick = Box<dyn FnOnce(usize)>;

    fn installs_ok(package: &Path, _: &Path, version: &str) -> Result<(), InstallError> {
        INSTALLS.with(|i| {
            i.borrow_mut()
                .push((package.to_path_buf(), version.to_string()))
        });
        Ok(())
    }

    fn setup(
        cx: &mut TestAppContext,
        manifest: Result<String, String>,
        install: InstallFn,
        click: Option<usize>,
    ) -> (Entity<Updater>, Arc<FakeHttp>, Rc<RefCell<Vec<bool>>>) {
        DIALOGS.with(|d| d.borrow_mut().clear());
        INSTALLS.with(|i| i.borrow_mut().clear());
        RELAUNCHES.with(|r| *r.borrow_mut() = 0);
        CLICK.with(|c| *c.borrow_mut() = click);
        PENDING.with(|p| *p.borrow_mut() = None);
        let http = Arc::new(FakeHttp {
            manifest,
            downloads: Mutex::new(0),
        });
        let busy = Rc::new(RefCell::new(Vec::new()));
        let busy_log = busy.clone();
        let updater = cx.new(|_| {
            let mut updater = Updater::new(
                http.clone(),
                native(install),
                Box::new(move |b| busy_log.borrow_mut().push(b)),
            );
            updater.app = Some(PathBuf::from("/Applications/Keylens.app"));
            // Tests run in parallel: each needs its own folder (an install deletes it).
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            updater.download_dir = std::env::temp_dir()
                .join(format!("keylens-updater-test-{}-{n}", std::process::id()));
            updater
        });
        (updater, http, busy)
    }

    fn dialog_titles() -> Vec<String> {
        DIALOGS.with(|d| d.borrow().iter().map(|d| d.title.clone()).collect())
    }

    #[gpui_kit::test]
    fn manual_check_says_when_up_to_date(cx: &mut TestAppContext) {
        let (updater, http, busy) = setup(cx, release_manifest(CURRENT), installs_ok, Some(0));
        updater.update(cx, |u, cx| u.check(true, cx));
        cx.run_until_parked();
        assert_eq!(dialog_titles(), [t!("Update.up_to_date_title").to_string()]);
        assert_eq!(*busy.borrow(), [true, false]);
        assert_eq!(*http.downloads.lock().unwrap(), 0);
    }

    #[gpui_kit::test]
    fn automatic_check_stays_quiet_without_an_update_or_on_errors(cx: &mut TestAppContext) {
        let (updater, _, _) = setup(cx, release_manifest(CURRENT), installs_ok, Some(0));
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        let (updater, _, _) = setup(cx, Err("offline".into()), installs_ok, Some(0));
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        assert!(dialog_titles().is_empty());
    }

    #[gpui_kit::test]
    fn manual_check_reports_errors(cx: &mut TestAppContext) {
        let (updater, _, busy) = setup(cx, Err("offline".into()), installs_ok, Some(0));
        updater.update(cx, |u, cx| u.check(true, cx));
        cx.run_until_parked();
        assert_eq!(dialog_titles(), [t!("Update.check_failed").to_string()]);
        assert_eq!(*busy.borrow(), [true, false]);
    }

    #[gpui_kit::test]
    fn later_installs_nothing(cx: &mut TestAppContext) {
        let (updater, http, _) = setup(cx, release_manifest("99.0.0"), installs_ok, Some(1));
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        assert_eq!(
            dialog_titles(),
            [t!("Update.available_title", version = "99.0.0").to_string()]
        );
        assert_eq!(*http.downloads.lock().unwrap(), 0);
        assert!(!updater.read_with(cx, |u, _| u.busy()));
    }

    #[gpui_kit::test]
    fn a_check_while_a_dialog_is_open_is_ignored(cx: &mut TestAppContext) {
        let (updater, http, _) = setup(cx, release_manifest("99.0.0"), installs_ok, None);
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        assert!(updater.read_with(cx, |u, _| u.busy()));
        updater.update(cx, |u, cx| u.check(true, cx));
        cx.run_until_parked();
        assert_eq!(dialog_titles().len(), 1);
        // The first dialog still works.
        let done = PENDING.with(|p| p.borrow_mut().take()).unwrap();
        done(0);
        cx.run_until_parked();
        assert_eq!(*http.downloads.lock().unwrap(), 1);
    }

    #[gpui_kit::test]
    fn install_downloads_installs_and_relaunches(cx: &mut TestAppContext) {
        let (updater, http, _) = setup(cx, release_manifest("99.0.0"), installs_ok, Some(0));
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        assert_eq!(*http.downloads.lock().unwrap(), 1);
        let installed = INSTALLS.with(|i| i.borrow().clone());
        assert_eq!(installed.len(), 1);
        assert!(installed[0].0.ends_with("Keylens-99.0.0.dmg"));
        assert_eq!(installed[0].1, "99.0.0");
        assert_eq!(RELAUNCHES.with(|r| *r.borrow()), 1);
    }

    #[gpui_kit::test]
    fn failed_install_explains_why_and_frees_the_menu(cx: &mut TestAppContext) {
        fn rejects(_: &Path, _: &Path, _: &str) -> Result<(), InstallError> {
            Err(InstallError::Signature("not ours".into()))
        }
        let (updater, _, busy) = setup(cx, release_manifest("99.0.0"), rejects, Some(0));
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        let last = DIALOGS.with(|d| d.borrow().last().cloned()).unwrap();
        assert_eq!(last.title, t!("Update.install_failed"));
        assert_eq!(last.message, t!("Update.err_signature"));
        assert_eq!(RELAUNCHES.with(|r| *r.borrow()), 0);
        assert_eq!(*busy.borrow(), [true, false]);
    }

    #[test]
    fn truncate_cuts_long_notes_on_a_char_boundary() {
        assert_eq!(truncate("短说明", 10), "短说明");
        assert_eq!(truncate("一二三四", 2), "一二…");
    }
}
