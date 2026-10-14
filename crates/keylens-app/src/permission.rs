//! Tracks Accessibility permission changes while Keylens runs (it can be revoked or granted
//! at any time in System Settings).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionChange {
    Granted,
    Revoked,
}

#[derive(Default)]
pub struct PermissionWatch {
    last: Option<bool>,
}

impl PermissionWatch {
    /// Feeds the current state; returns a change on the first call and whenever it flips.
    pub fn update(&mut self, granted: bool) -> Option<PermissionChange> {
        if self.last == Some(granted) {
            return None;
        }
        self.last = Some(granted);
        Some(if granted {
            PermissionChange::Granted
        } else {
            PermissionChange::Revoked
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_initial_state_and_then_only_changes() {
        let mut watch = PermissionWatch::default();
        assert_eq!(watch.update(false), Some(PermissionChange::Revoked));
        assert_eq!(watch.update(false), None);
        assert_eq!(watch.update(true), Some(PermissionChange::Granted));
        assert_eq!(watch.update(true), None);
        assert_eq!(watch.update(false), Some(PermissionChange::Revoked));
    }
}
