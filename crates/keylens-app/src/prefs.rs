//! `Prefs`: the one owner of `config.toml`. The settings window edits it; the controller, tray and
//! settings window observe it. Hand edits to the file are picked up by polling.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{Context, Task};
use keylens_core::{Locale, Os, Trigger};

use crate::config::{Config, TriggerSettings, UpdateSettings, choose_locale, parse_config};
use crate::config_write::{self, Change};
use crate::watch::{self, Stamp};

const POLL: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    /// The file does not parse. The last good config stays in effect and nothing is written.
    Invalid(String),
    /// The last write failed. Changes are in effect for this run only.
    WriteFailed(String),
}

pub struct Prefs {
    path: PathBuf,
    system_locales: Vec<String>,
    default_trigger: Trigger,
    os: Os,
    /// The text `config` was parsed from; edits apply to it, so changes that failed to write are
    /// kept and written with the next successful save.
    text: String,
    config: Config,
    problem: Option<Problem>,
    stamp: Option<Stamp>,
    _watch: Option<Task<()>>,
}

impl Prefs {
    pub fn load(
        path: PathBuf,
        system_locales: Vec<String>,
        default_trigger: Trigger,
        os: Os,
    ) -> Self {
        let stamp = watch::file_stamp(&path);
        let mut prefs = Self {
            path,
            system_locales,
            default_trigger,
            os,
            text: String::new(),
            config: Config::default(),
            problem: None,
            stamp,
            _watch: None,
        };
        match prefs.read_file() {
            Ok(text) => prefs.adopt_text(text),
            Err(problem) => {
                tracing::warn!("{problem:?}; using defaults");
                prefs.problem = Some(problem);
            }
        }
        prefs
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn problem(&self) -> Option<&Problem> {
        self.problem.as_ref()
    }

    /// True while the file does not parse: writing would destroy what the user typed.
    pub fn read_only(&self) -> bool {
        matches!(self.problem, Some(Problem::Invalid(_)))
    }

    pub fn locale(&self) -> Locale {
        choose_locale(&self.config, &self.system_locales)
    }

    pub fn trigger_settings(&self) -> TriggerSettings {
        self.config.trigger_settings(&self.default_trigger).0
    }

    pub fn update_settings(&self) -> UpdateSettings {
        self.config.update_settings().0
    }

    pub fn default_trigger_settings(&self) -> TriggerSettings {
        TriggerSettings::defaults(&self.default_trigger)
    }

    /// Applies one edit: updates memory and writes the file. Setting a value equal to the default
    /// removes its key. Does nothing while the file is invalid or when nothing would change.
    pub fn set(&mut self, change: Change, cx: &mut Context<Self>) {
        // Pick up hand edits made since the last poll, so the write does not start from stale text.
        self.reload(cx);
        if self.read_only() {
            return;
        }
        let change = self.normalize(change);
        let text = match config_write::apply(&self.text, &change, self.os) {
            Ok(text) => text,
            Err(error) => {
                self.problem = Some(Problem::Invalid(format!(
                    "{}: {error}",
                    self.path.display()
                )));
                cx.notify();
                return;
            }
        };
        if text == self.text {
            return;
        }
        self.adopt_text(text);
        self.problem = match config_write::write_atomic(&self.path, &self.text) {
            Ok(()) => None,
            Err(error) => {
                tracing::warn!(%error, "cannot save {}", self.path.display());
                Some(Problem::WriteFailed(error.to_string()))
            }
        };
        self.stamp = watch::file_stamp(&self.path);
        cx.notify();
    }

    /// Re-reads the file if it changed on disk since the last read or write.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let stamp = watch::file_stamp(&self.path);
        if stamp == self.stamp {
            return;
        }
        self.stamp = stamp;
        match self.read_file() {
            Ok(text) => {
                let before = (self.config.clone(), self.problem.clone());
                if matches!(self.problem, Some(Problem::WriteFailed(_))) {
                    tracing::warn!("config.toml on disk replaced settings that could not be saved");
                }
                self.adopt_text(text);
                self.problem = None;
                if before != (self.config.clone(), None) {
                    tracing::info!("config.toml reloaded");
                    cx.notify();
                }
            }
            Err(problem) => {
                tracing::warn!("{problem:?}; keeping the previous settings");
                self.problem = Some(problem);
                cx.notify();
            }
        }
    }

    /// Polls the file every 2 seconds until this entity is dropped.
    pub fn watch(&mut self, cx: &mut Context<Self>) {
        self._watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                    break;
                }
            }
        }));
    }

    /// Reads and validates the file. A missing file is an empty config.
    fn read_file(&self) -> Result<String, Problem> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(Problem::Invalid(format!("{}: {e}", self.path.display()))),
        };
        parse_config(&text)
            .map(|_| text)
            .map_err(|e| Problem::Invalid(format!("{}: {e}", self.path.display())))
    }

    /// Takes `text` (already known to parse) as the current config and logs its warnings.
    fn adopt_text(&mut self, text: String) {
        self.config = parse_config(&text).unwrap_or_default();
        self.text = text;
        let (_, mut warnings) = self.config.trigger_settings(&self.default_trigger);
        warnings.extend(self.config.update_settings().1);
        for warning in warnings {
            tracing::warn!("config.toml: {warning}");
        }
    }

    fn normalize(&self, change: Change) -> Change {
        let d = self.default_trigger_settings();
        match change {
            Change::Mode(Some(m)) if m == d.mode => Change::Mode(None),
            Change::Key(Some(k)) if k == d.key => Change::Key(None),
            Change::HoldMs(Some(ms)) if ms == d.hold_ms => Change::HoldMs(None),
            Change::DoubleTapMs(Some(ms)) if ms == d.double_tap_ms => Change::DoubleTapMs(None),
            Change::AutoCheck(Some(on)) if on == UpdateSettings::default().auto_check => {
                Change::AutoCheck(None)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    // See view.rs: keep the built-in #[test] in scope next to gpui's.
    use core::prelude::v1::test;
    use std::path::PathBuf;

    use gpui_kit::{AppContext as _, Entity, TestAppContext};
    use keylens_core::{Lang, Modifiers, Os, Trigger};

    use super::*;
    use crate::config::TriggerMode;
    use crate::config_write::Change;

    fn temp_config(name: &str, text: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keylens-prefs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        if let Some(text) = text {
            std::fs::write(&path, text).unwrap();
        }
        path
    }

    fn mac_default() -> Trigger {
        Trigger::HoldModifier {
            key: Modifiers::META,
            delay: Trigger::DEFAULT_HOLD_DELAY,
        }
    }

    fn prefs(cx: &mut TestAppContext, path: &Path) -> Entity<Prefs> {
        let prefs = Prefs::load(
            path.to_path_buf(),
            vec!["en-US".into()],
            mac_default(),
            Os::MacOS,
        );
        cx.new(|_| prefs)
    }

    /// Rewrites the file and makes sure its stamp changes even on coarse-mtime filesystems.
    fn rewrite(path: &Path, text: &str) {
        std::fs::write(
            path,
            format!("{text}\n# {}\n", "x".repeat(text.len() % 7 + 1)),
        )
        .unwrap();
    }

    #[test]
    fn missing_file_is_the_default_without_a_problem() {
        let path = temp_config("missing", None);
        let p = Prefs::load(path, vec![], mac_default(), Os::MacOS);
        assert_eq!(p.config(), &Config::default());
        assert!(p.problem().is_none());
        assert_eq!(p.trigger_settings(), p.default_trigger_settings());
    }

    #[test]
    fn invalid_file_at_launch_is_read_only() {
        let path = temp_config("broken", Some("language = "));
        let p = Prefs::load(path, vec![], mac_default(), Os::MacOS);
        assert!(p.read_only());
        assert!(matches!(p.problem(), Some(Problem::Invalid(e)) if e.contains("config.toml")));
        assert_eq!(p.config(), &Config::default());
    }

    #[test]
    fn locale_prefers_the_config_over_the_system() {
        let path = temp_config("locale", Some("language = \"ja\""));
        let p = Prefs::load(path, vec!["zh-Hans-CN".into()], mac_default(), Os::MacOS);
        assert_eq!(p.locale().lang, Lang::Ja);
    }

    #[gpui_kit::test]
    fn set_writes_the_file_and_updates_memory(cx: &mut TestAppContext) {
        let path = temp_config("set", Some("# keep me\n"));
        let prefs = prefs(cx, &path);
        prefs.update(cx, |p, cx| {
            p.set(Change::Mode(Some(TriggerMode::DoubleTap)), cx)
        });
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep me"));
        assert!(text.contains("mode = \"double-tap\""));
        prefs.read_with(cx, |p, _| {
            assert_eq!(p.trigger_settings().mode, TriggerMode::DoubleTap);
            assert!(p.problem().is_none());
        });
    }

    #[gpui_kit::test]
    fn setting_the_default_value_removes_the_key(cx: &mut TestAppContext) {
        let path = temp_config("default", Some("[trigger]\nhold-ms = 1200\n"));
        let prefs = prefs(cx, &path);
        prefs.update(cx, |p, cx| p.set(Change::HoldMs(Some(800)), cx));
        assert!(!std::fs::read_to_string(&path).unwrap().contains("hold-ms"));
    }

    #[gpui_kit::test]
    fn setting_the_same_value_does_not_touch_the_file(cx: &mut TestAppContext) {
        let path = temp_config("same", Some("[trigger]\nhold-ms = 1200\n"));
        let prefs = prefs(cx, &path);
        let notified = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = notified.clone();
        let _sub = cx.update(|cx| cx.observe(&prefs, move |_, _| count.set(count.get() + 1)));
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        prefs.update(cx, |p, cx| p.set(Change::HoldMs(Some(1200)), cx));
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
        assert_eq!(notified.get(), 0);
    }

    #[gpui_kit::test]
    fn external_edits_are_picked_up(cx: &mut TestAppContext) {
        let path = temp_config("external", Some(""));
        let prefs = prefs(cx, &path);
        rewrite(&path, "[trigger]\nkey = \"shift\"");
        prefs.update(cx, |p, cx| p.reload(cx));
        prefs.read_with(cx, |p, _| {
            assert_eq!(p.trigger_settings().key, Modifiers::SHIFT)
        });
    }

    #[gpui_kit::test]
    fn invalid_file_keeps_last_config_and_blocks_writes(cx: &mut TestAppContext) {
        let path = temp_config("invalid", Some("[trigger]\nkey = \"shift\"\n"));
        let prefs = prefs(cx, &path);
        rewrite(&path, "[trigger\nkey = ");
        prefs.update(cx, |p, cx| p.reload(cx));
        prefs.update(cx, |p, cx| p.set(Change::Language(Some("ja".into())), cx));
        prefs.read_with(cx, |p, _| {
            assert!(p.read_only());
            assert_eq!(p.trigger_settings().key, Modifiers::SHIFT);
            assert_eq!(p.config().language, None);
        });
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("[trigger\n")
        );
    }

    #[gpui_kit::test]
    fn fixing_the_file_clears_the_problem(cx: &mut TestAppContext) {
        let path = temp_config("fixed", Some("language = "));
        let prefs = prefs(cx, &path);
        rewrite(&path, "language = \"en\"");
        prefs.update(cx, |p, cx| p.reload(cx));
        prefs.read_with(cx, |p, _| {
            assert!(!p.read_only());
            assert_eq!(p.config().language.as_deref(), Some("en"));
        });
    }

    #[gpui_kit::test]
    fn set_keeps_hand_edits_made_since_the_last_poll(cx: &mut TestAppContext) {
        let path = temp_config("handedit", Some("# old\n"));
        let prefs = prefs(cx, &path);
        rewrite(&path, "# hand edit\nlanguage = \"ja\"");
        prefs.update(cx, |p, cx| p.set(Change::HoldMs(Some(1000)), cx));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# hand edit"), "{text}");
        assert!(text.contains("language = \"ja\""), "{text}");
        assert!(text.contains("hold-ms = 1000"), "{text}");
    }

    #[gpui_kit::test]
    fn set_does_not_overwrite_a_hand_edit_that_broke_the_file(cx: &mut TestAppContext) {
        let path = temp_config("handbroken", Some("# old\n"));
        let prefs = prefs(cx, &path);
        rewrite(&path, "[trigger\nkey = ");
        let broken = std::fs::read_to_string(&path).unwrap();
        prefs.update(cx, |p, cx| p.set(Change::HoldMs(Some(1000)), cx));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        prefs.read_with(cx, |p, _| assert!(p.read_only()));
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    fn write_failure_keeps_changes_in_memory(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_config("readonly", Some(""));
        let dir = path.parent().unwrap().to_path_buf();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        // Root ignores directory permissions (e.g. in Docker); nothing to test then.
        let probe = dir.join("probe");
        if std::fs::write(&probe, "").is_ok() {
            std::fs::remove_file(&probe).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }
        let prefs = prefs(cx, &path);
        prefs.update(cx, |p, cx| p.set(Change::Key(Some(Modifiers::ALT)), cx));
        prefs.update(cx, |p, cx| p.set(Change::HoldMs(Some(1000)), cx));
        prefs.read_with(cx, |p, _| {
            assert!(matches!(p.problem(), Some(Problem::WriteFailed(_))));
            assert!(!p.read_only());
            let s = p.trigger_settings();
            assert_eq!((s.key, s.hold_ms), (Modifiers::ALT, 1000));
        });
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        prefs.update(cx, |p, cx| p.set(Change::Language(Some("en".into())), cx));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("key = \"option\"") && text.contains("hold-ms = 1000"),
            "{text}"
        );
        prefs.read_with(cx, |p, _| assert!(p.problem().is_none()));
    }
}
