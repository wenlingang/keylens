//! Tracks Accessibility permission while Keylens runs (it can be revoked or granted at any time
//! in System Settings) and decides when to (re)start the key listener.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionAction {
    /// Permission was revoked (or is missing at launch): stop listening, show the guide.
    Revoked,
    /// Permission is granted but nothing is listening: try to start. Repeated on every check
    /// until it succeeds, because creating the event tap can fail right after a grant.
    StartListening,
}

#[derive(Default)]
pub struct PermissionWatch {
    granted: Option<bool>,
}

impl PermissionWatch {
    /// Feeds the current permission and whether the key listener is running.
    pub fn step(&mut self, granted: bool, listening: bool) -> Vec<PermissionAction> {
        let changed = self.granted != Some(granted);
        self.granted = Some(granted);
        match (granted, listening) {
            (false, _) if changed => vec![PermissionAction::Revoked],
            (true, false) => vec![PermissionAction::StartListening],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revocation_is_reported_once() {
        let mut watch = PermissionWatch::default();
        assert_eq!(watch.step(false, false), [PermissionAction::Revoked]);
        assert_eq!(watch.step(false, false), []);
    }

    #[test]
    fn listening_is_retried_until_it_starts() {
        let mut watch = PermissionWatch::default();
        assert_eq!(watch.step(true, false), [PermissionAction::StartListening]);
        // Starting failed: try again on the next tick even though nothing changed.
        assert_eq!(watch.step(true, false), [PermissionAction::StartListening]);
        assert_eq!(watch.step(true, true), []);
    }

    #[test]
    fn revoke_then_regrant_restarts_listening() {
        let mut watch = PermissionWatch::default();
        watch.step(true, false);
        assert_eq!(watch.step(true, true), []);
        assert_eq!(watch.step(false, true), [PermissionAction::Revoked]);
        assert_eq!(watch.step(false, false), []);
        assert_eq!(watch.step(true, false), [PermissionAction::StartListening]);
    }
}
