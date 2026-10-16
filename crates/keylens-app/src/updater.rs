//! In-app updates: checks for a new release on a timer and from the menu bar, asks before
//! installing, downloads and installs it, then relaunches. The steps live in `update` (check and
//! download) and the platform's `UpdateNative` (install, relaunch, dialogs). `[update]` in
//! config.toml turns automatic checks off and remembers a skipped version.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AsyncApp, Context, Entity, Task, WeakEntity};
use keylens_platform::update::{Dialog, InstallError, UpdateNative, bundle_of};
use rust_i18n::t;

use crate::config_write::Change;
use crate::prefs::Prefs;
use crate::update::{self, Http, Release, UpdateError};

/// Delay before the first automatic check, so it does not compete with startup.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Release notes longer than this are cut in the dialog.
const NOTES_LIMIT: usize = 1000;

/// Buttons of the "new version" dialog, in order (the first is the default).
const INSTALL: usize = 0;
const LATER: usize = 1;
const SKIP: usize = 2;

/// Why an update failed, for the error dialog.
#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error(transparent)]
    Update(#[from] UpdateError),
    #[error(transparent)]
    Install(#[from] InstallError),
}

/// Gets what the updater is doing, for the menu bar; `None` once idle.
pub type OnStatus = Box<dyn Fn(Option<&str>)>;

pub struct Updater {
    http: Arc<dyn Http>,
    native: UpdateNative,
    prefs: Entity<Prefs>,
    /// Where the running app is installed; `None` when not running from an app bundle.
    app: Option<PathBuf>,
    /// Folder for the downloaded .dmg.
    download_dir: PathBuf,
    /// Told what the updater is doing (e.g. "Downloading 45%"), or `None` once it is idle.
    on_status: OnStatus,
    /// The version updates are compared with: this build's, unless overridden for testing.
    current: String,
    /// A check or install is running, or one of its dialogs is open.
    busy: bool,
    schedule: Option<Task<()>>,
    job: Option<Task<()>>,
}

impl Updater {
    pub fn new(
        http: Arc<dyn Http>,
        native: UpdateNative,
        prefs: Entity<Prefs>,
        on_status: OnStatus,
    ) -> Self {
        Self {
            http,
            native,
            prefs,
            app: std::env::current_exe().ok().and_then(|exe| bundle_of(&exe)),
            download_dir: std::env::temp_dir().join("keylens-update"),
            on_status,
            current: CURRENT.to_string(),
            busy: false,
            schedule: None,
            job: None,
        }
    }

    /// Checks shortly after startup and then once a day, telling the user only about new versions.
    /// Each check reads `[update] auto-check` at that moment, so turning it off takes effect at once.
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

    /// Compares releases with `version` instead of this build's, so a development build can show
    /// the update dialog and progress for the current release.
    pub fn pretend_version(&mut self, version: String) {
        self.current = version;
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Checks for a new version. `manual` (from the menu) also reports "up to date" and errors, and
    /// offers a skipped version again; automatic checks only log them and do nothing when turned
    /// off. Stays busy until the last dialog is answered, so a second check cannot replace (and
    /// cancel) a job that is waiting for the user.
    pub fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let settings = self.prefs.read(cx).update_settings();
        if !manual && !settings.auto_check {
            return;
        }
        self.set_status(Some(t!("Tray.checking").to_string()));
        let http = self.http.clone();
        let current = self.current.clone();
        self.job = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { update::check(&*http, &current) })
                .await;
            match result {
                Ok(Some(release))
                    if !manual
                        && settings.skip_version.as_deref()
                            == Some(release.version.to_string().as_str()) =>
                {
                    tracing::info!(version = %release.version, "update available but skipped");
                }
                Ok(Some(release)) => {
                    tracing::info!(version = %release.version, "update available");
                    let dialog =
                        this.update(cx, |this, _| available_dialog(&release, &this.current));
                    let Ok(dialog) = dialog else { return };
                    match ask(&this, dialog, cx).await {
                        Some(INSTALL) => {
                            // Runs on as this job: `install` must not replace `self.job` from
                            // inside it.
                            install_release(&this, release, cx).await;
                            return;
                        }
                        Some(SKIP) => {
                            tracing::info!(version = %release.version, "update skipped");
                            let change = Change::SkipVersion(Some(release.version.to_string()));
                            let _ = this.update(cx, |this, cx| {
                                this.prefs.update(cx, |p, cx| p.set(change, cx))
                            });
                        }
                        _ => {}
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
            let _ = this.update(cx, |this, _| this.set_status(None));
        }));
    }

    fn set_status(&mut self, status: Option<String>) {
        self.busy = status.is_some();
        (self.on_status)(status.as_deref());
    }
}

/// Downloads and installs `release`, showing the progress in the menu bar, then relaunches
/// (quitting this process); on failure tells the user why and ends the busy state.
async fn install_release(this: &WeakEntity<Updater>, release: Release, cx: &mut AsyncApp) {
    let Ok((http, native, app, dir)) = this.update(cx, |this, _| {
        this.set_status(Some(downloading_status(0, release.size)));
        (
            this.http.clone(),
            this.native,
            this.app.clone(),
            this.download_dir.clone(),
        )
    }) else {
        return;
    };

    // The download reports bytes from a background thread; the menu bar is updated on the main
    // thread. The channel closes when the download task ends.
    let (progress_tx, progress_rx) = async_channel::unbounded::<Option<String>>();
    let watcher = {
        let this = this.clone();
        cx.spawn(async move |cx| {
            while let Ok(status) = progress_rx.recv().await {
                let _ = this.update(cx, |this, _| this.set_status(status));
            }
        })
    };
    let size = release.size;
    let result = cx
        .background_executor()
        .spawn(async move {
            let app = app.ok_or(InstallError::NotInApp)?;
            let last = std::cell::Cell::new(None);
            let progress = |bytes: u64| {
                let status = downloading_status(bytes, size);
                if last.replace(Some(status.clone())).as_ref() != Some(&status) {
                    let _ = progress_tx.try_send(Some(status));
                }
            };
            let package = update::download(&*http, &release, &dir, &progress)?;
            let _ = progress_tx.try_send(Some(t!("Tray.installing").to_string()));
            drop(progress_tx);
            let installed = (native.install)(&package, &app, &release.version.to_string());
            let _ = std::fs::remove_dir_all(&dir);
            installed?;
            Ok::<_, Failure>(app)
        })
        .await;
    watcher.await;
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
    let _ = this.update(cx, |this, _| this.set_status(None));
}

/// "Downloading 45%" when the size is known, otherwise "Downloading 12.3 MB".
fn downloading_status(bytes: u64, size: Option<u64>) -> String {
    match size {
        Some(size) => {
            let percent = (bytes.min(size) * 100 / size).to_string();
            t!("Tray.downloading_percent", percent = percent).to_string()
        }
        None => {
            let mb = format!("{:.1}", bytes as f64 / 1_000_000.0);
            t!("Tray.downloading_mb", mb = mb).to_string()
        }
    }
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

fn available_dialog(release: &Release, current: &str) -> Dialog {
    let mut buttons = vec![String::new(); 3];
    buttons[INSTALL] = t!("Update.install").to_string();
    buttons[LATER] = t!("Update.later").to_string();
    buttons[SKIP] = t!("Update.skip").to_string();
    Dialog {
        title: t!("Update.available_title", version = release.version).to_string(),
        message: t!("Update.available_body", current = current).to_string(),
        details: format_notes(&release.notes),
        buttons,
        cancel: Some(LATER),
    }
}

/// Release notes for the dialog: drops a first line that only repeats the app name and version
/// (the dialog's title says it), turns "- " and "* " list items into bullets, cuts very long notes.
/// `None` when nothing is left.
fn format_notes(notes: &str) -> Option<String> {
    let mut lines: Vec<&str> = notes.lines().collect();
    if lines
        .first()
        .is_some_and(|line| line.trim().starts_with("Keylens "))
    {
        lines.remove(0);
    }
    let text = lines
        .iter()
        .map(|line| {
            let trimmed = line.trim_start();
            match trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                Some(item) => format!("• {item}"),
                None => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| truncate(text, NOTES_LIMIT))
}

fn notice(title: String, message: String) -> Dialog {
    Dialog {
        title,
        message,
        details: None,
        buttons: vec![t!("Update.ok").to_string()],
        cancel: None,
    }
}

fn up_to_date_dialog() -> Dialog {
    notice(
        t!("Update.up_to_date_title").to_string(),
        t!("Update.up_to_date_body", version = CURRENT).to_string(),
    )
}

fn restart_dialog() -> Dialog {
    notice(
        t!("Update.installed_title").to_string(),
        t!("Update.installed_body").to_string(),
    )
}

fn failed_dialog(title: impl Into<String>, failure: &Failure) -> Dialog {
    notice(title.into(), failure_message(failure))
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

    use gpui_kit::{AppContext, TestAppContext};
    use keylens_core::{Modifiers, Os, Trigger};

    use super::*;

    const SHA_OF_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    struct FakeHttp {
        manifest: Result<String, String>,
        checks: Mutex<usize>,
        downloads: Mutex<usize>,
    }

    impl Http for FakeHttp {
        fn get(&self, _: &str) -> Result<Vec<u8>, String> {
            *self.checks.lock().unwrap() += 1;
            self.manifest.clone().map(String::into_bytes)
        }
        fn download(&self, _: &str, to: &Path, progress: &dyn Fn(u64)) -> Result<(), String> {
            *self.downloads.lock().unwrap() += 1;
            progress(0);
            progress(500);
            std::fs::write(to, b"").map_err(|e| e.to_string())
        }
    }

    fn release_manifest(version: &str) -> Result<String, String> {
        Ok(format!(
            r#"{{"version":"{version}","dmg":"Keylens-{version}.dmg","sha256":"{SHA_OF_EMPTY}","size":1000,"notes":"Keylens {version}\n\n- New things"}}"#
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
    type StatusLog = Rc<RefCell<Vec<Option<String>>>>;

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
    ) -> (Entity<Updater>, Arc<FakeHttp>, StatusLog) {
        DIALOGS.with(|d| d.borrow_mut().clear());
        INSTALLS.with(|i| i.borrow_mut().clear());
        RELAUNCHES.with(|r| *r.borrow_mut() = 0);
        CLICK.with(|c| *c.borrow_mut() = click);
        PENDING.with(|p| *p.borrow_mut() = None);
        let http = Arc::new(FakeHttp {
            manifest,
            checks: Mutex::new(0),
            downloads: Mutex::new(0),
        });
        // Tests run in parallel: each needs its own folders (an install deletes its download one).
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("keylens-updater-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prefs = cx.new(|_| {
            Prefs::load(
                dir.join("config.toml"),
                vec![],
                Trigger::HoldModifier {
                    key: Modifiers::META,
                    delay: Trigger::DEFAULT_HOLD_DELAY,
                },
                Os::MacOS,
            )
        });
        let statuses = Rc::new(RefCell::new(Vec::new()));
        let log = statuses.clone();
        let updater = cx.new(|_| {
            let mut updater = Updater::new(
                http.clone(),
                native(install),
                prefs,
                Box::new(move |status| log.borrow_mut().push(status.map(str::to_string))),
            );
            updater.app = Some(PathBuf::from("/Applications/Keylens.app"));
            updater.download_dir = dir.join("download");
            updater
        });
        (updater, http, statuses)
    }

    fn set_pref(updater: &Entity<Updater>, change: Change, cx: &mut TestAppContext) {
        updater.update(cx, |u, cx| u.prefs.update(cx, |p, cx| p.set(change, cx)));
    }

    fn checking() -> Option<String> {
        Some(t!("Tray.checking").to_string())
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
        assert_eq!(*busy.borrow(), [checking(), None]);
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
        assert_eq!(*busy.borrow(), [checking(), None]);
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
        let statuses = busy.borrow();
        assert_eq!(statuses.first(), Some(&checking()));
        assert_eq!(statuses.last(), Some(&None));
        assert!(!updater.read_with(cx, |u, _| u.busy()));
    }

    #[gpui_kit::test]
    fn install_shows_download_progress_then_installing(cx: &mut TestAppContext) {
        let (updater, _, statuses) = setup(cx, release_manifest("99.0.0"), installs_ok, Some(0));
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        let statuses = statuses.borrow();
        let percent = |p: &str| Some(t!("Tray.downloading_percent", percent = p).to_string());
        for want in [
            percent("0"),
            percent("50"),
            Some(t!("Tray.installing").to_string()),
        ] {
            assert!(statuses.contains(&want), "{want:?} not in {statuses:?}");
        }
    }

    #[gpui_kit::test]
    fn the_dialog_shows_notes_apart_with_three_buttons(cx: &mut TestAppContext) {
        let (updater, _, _) = setup(cx, release_manifest("99.0.0"), installs_ok, Some(LATER));
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        let dialog = DIALOGS.with(|d| d.borrow()[0].clone());
        assert_eq!(dialog.details.as_deref(), Some("• New things"));
        assert_eq!(dialog.buttons.len(), 3);
        assert_eq!(dialog.cancel, Some(LATER));
    }

    #[gpui_kit::test]
    fn automatic_checks_can_be_turned_off(cx: &mut TestAppContext) {
        let (updater, http, statuses) =
            setup(cx, release_manifest("99.0.0"), installs_ok, Some(LATER));
        set_pref(&updater, Change::AutoCheck(Some(false)), cx);
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        assert_eq!(*http.checks.lock().unwrap(), 0);
        assert!(statuses.borrow().is_empty());
        // The menu still checks.
        updater.update(cx, |u, cx| u.check(true, cx));
        cx.run_until_parked();
        assert_eq!(dialog_titles().len(), 1);
    }

    #[gpui_kit::test]
    fn skip_this_version_hides_it_from_automatic_checks_only(cx: &mut TestAppContext) {
        let (updater, _, _) = setup(cx, release_manifest("99.0.0"), installs_ok, Some(SKIP));
        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        let skipped =
            updater.read_with(cx, |u, cx| u.prefs.read(cx).update_settings().skip_version);
        assert_eq!(skipped.as_deref(), Some("99.0.0"));

        updater.update(cx, |u, cx| u.check(false, cx));
        cx.run_until_parked();
        assert_eq!(
            dialog_titles().len(),
            1,
            "an automatic check offered a skipped version"
        );

        updater.update(cx, |u, cx| u.check(true, cx));
        cx.run_until_parked();
        assert_eq!(dialog_titles().len(), 2, "the menu should still offer it");
    }

    #[test]
    fn format_notes_drops_the_title_line_and_makes_bullets() {
        assert_eq!(
            format_notes("Keylens 0.1.2\n\n- One\n* Two\nPlain").as_deref(),
            Some("• One\n• Two\nPlain")
        );
        assert_eq!(format_notes("Keylens 0.1.2\n"), None);
        assert_eq!(format_notes(""), None);
        assert_eq!(format_notes("- Only").as_deref(), Some("• Only"));
    }

    #[test]
    fn truncate_cuts_long_notes_on_a_char_boundary() {
        assert_eq!(truncate("短说明", 10), "短说明");
        assert_eq!(truncate("一二三四", 2), "一二…");
    }
}
