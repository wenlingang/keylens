//! Panel session state: whether the panel is shown and which app it targets. Pure logic; the
//! controller turns the returned effects into reads, presentation and hiding.

use keylens_core::AppInfo;

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Prefetch(AppInfo),
    Present(AppInfo),
    Hide,
}

pub struct Session {
    own_pid: i32,
    visible: bool,
    target: Option<AppInfo>,
}

impl Session {
    pub fn new(own_pid: i32) -> Self {
        Self {
            own_pid,
            visible: false,
            target: None,
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn target(&self) -> Option<&AppInfo> {
        self.target.as_ref()
    }

    fn other(&self, app: Option<AppInfo>) -> Option<AppInfo> {
        app.filter(|a| a.pid != self.own_pid)
    }

    pub fn on_prefetch(&mut self, frontmost: Option<AppInfo>) -> Vec<Effect> {
        if self.visible {
            return Vec::new();
        }
        match self.other(frontmost) {
            Some(app) => {
                self.target = Some(app.clone());
                vec![Effect::Prefetch(app)]
            }
            None => Vec::new(),
        }
    }

    pub fn on_show(&mut self, frontmost: Option<AppInfo>) -> Vec<Effect> {
        if let Some(app) = self.target.clone() {
            self.visible = true;
            return vec![Effect::Present(app)];
        }
        // No prefetched target (e.g. the ⌘ press started over Keylens itself): fall back to the
        // frontmost app and read it now, or the panel would wait forever.
        let Some(app) = self.other(frontmost) else {
            return Vec::new();
        };
        self.target = Some(app.clone());
        self.visible = true;
        vec![Effect::Present(app.clone()), Effect::Prefetch(app)]
    }

    pub fn on_cancel(&mut self) {
        if !self.visible {
            self.target = None;
        }
    }

    pub fn on_frontmost(&mut self, app: AppInfo) -> Vec<Effect> {
        if app.pid == self.own_pid {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if self.visible {
            if self.target.as_ref().map(|t| t.pid) != Some(app.pid) {
                effects.push(Effect::Hide);
            }
        } else {
            self.target = None;
        }
        effects.push(Effect::Prefetch(app));
        effects
    }

    pub fn on_hidden(&mut self) {
        self.visible = false;
        self.target = None;
    }

    /// The permission guide is shown: visible, but not for any app.
    pub fn on_permission_page(&mut self) {
        self.visible = true;
        self.target = None;
    }

    pub fn shows(&self, pid: i32) -> bool {
        self.visible && self.target.as_ref().map(|t| t.pid) == Some(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: i32 = 99;

    fn app(pid: i32) -> AppInfo {
        AppInfo {
            pid,
            name: format!("app{pid}"),
            ids: vec![],
        }
    }

    #[test]
    fn prefetch_targets_the_frontmost_app_but_never_ourselves() {
        let mut s = Session::new(OWN);
        assert_eq!(s.on_prefetch(Some(app(OWN))), []);
        assert_eq!(s.on_prefetch(None), []);
        assert_eq!(s.on_prefetch(Some(app(1))), [Effect::Prefetch(app(1))]);
        assert_eq!(s.target().map(|a| a.pid), Some(1));
    }

    #[test]
    fn show_uses_the_target_or_the_frontmost_app() {
        let mut s = Session::new(OWN);
        s.on_prefetch(Some(app(1)));
        assert_eq!(s.on_show(Some(app(2))), [Effect::Present(app(1))]);
        assert!(s.visible());
        // Falling back to the frontmost app also reads it: nothing was prefetched for it.
        let mut s = Session::new(OWN);
        assert_eq!(
            s.on_show(Some(app(2))),
            [Effect::Present(app(2)), Effect::Prefetch(app(2))]
        );
        let mut s = Session::new(OWN);
        assert_eq!(s.on_show(Some(app(OWN))), []);
        assert!(!s.visible());
    }

    #[test]
    fn cancel_clears_a_stale_target() {
        let mut s = Session::new(OWN);
        s.on_prefetch(Some(app(1)));
        s.on_cancel();
        assert_eq!(
            s.on_show(Some(app(2))),
            [Effect::Present(app(2)), Effect::Prefetch(app(2))]
        );
    }

    #[test]
    fn switching_apps_while_hidden_forgets_the_old_target() {
        let mut s = Session::new(OWN);
        s.on_prefetch(Some(app(1)));
        assert_eq!(s.on_frontmost(app(2)), [Effect::Prefetch(app(2))]);
        assert_eq!(
            s.on_show(Some(app(2))),
            [Effect::Present(app(2)), Effect::Prefetch(app(2))]
        );
    }

    #[test]
    fn switching_apps_while_visible_hides_the_panel() {
        let mut s = Session::new(OWN);
        s.on_show(Some(app(1)));
        assert_eq!(s.on_frontmost(app(OWN)), []);
        assert!(s.visible());
        assert_eq!(s.on_frontmost(app(1)), [Effect::Prefetch(app(1))]);
        assert_eq!(
            s.on_frontmost(app(2)),
            [Effect::Hide, Effect::Prefetch(app(2))]
        );
    }

    #[test]
    fn loaded_data_updates_the_view_only_for_the_shown_app() {
        let mut s = Session::new(OWN);
        s.on_show(Some(app(1)));
        assert!(s.shows(1));
        assert!(!s.shows(2));
        s.on_hidden();
        assert!(!s.shows(1));
        assert!(s.target().is_none());
    }

    #[test]
    fn prefetch_is_ignored_while_visible() {
        let mut s = Session::new(OWN);
        s.on_show(Some(app(1)));
        assert_eq!(s.on_prefetch(Some(app(2))), []);
    }

    #[test]
    fn permission_page_is_visible_without_target() {
        let mut s = Session::new(OWN);
        s.on_prefetch(Some(app(1)));
        s.on_permission_page();
        assert!(s.visible());
        assert!(s.target().is_none());
        assert!(!s.shows(1));
    }
}
