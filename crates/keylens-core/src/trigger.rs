//! Decides whether the user is invoking Keylens. A pure state machine; time is supplied by the caller.

use std::time::{Duration, Instant};

use crate::keys::{Chord, Modifiers};

#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    /// Long-press a modifier key. macOS default: ⌘, 800ms.
    HoldModifier { key: Modifiers, delay: Duration },
    /// Double-tap a modifier key. Windows / X11 default (wired up from v0.2).
    DoubleTap { key: Modifiers, interval: Duration },
    /// Key combination; the platform registers a global hotkey to handle it (reserved).
    Hotkey(Chord),
    /// Triggered by the external command `keylens toggle` (reserved).
    External,
}

impl Trigger {
    pub const DEFAULT_HOLD_DELAY: Duration = Duration::from_millis(800);
    pub const DEFAULT_DOUBLE_TAP_INTERVAL: Duration = Duration::from_millis(350);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEvent {
    /// Modifier state changed; carries all modifiers currently held.
    ModifiersChanged(Modifiers),
    KeyDown,
    MouseDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerAction {
    /// Start prefetching the frontmost app's shortcuts in the background.
    Prefetch,
    /// Show the panel.
    Show,
    /// Abandon this trigger.
    Cancel,
    /// The held key that showed the panel was released (hold mode only).
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Idle,
    Holding {
        since: Instant,
    },
    TapDown {
        since: Instant,
    },
    WaitSecond {
        deadline: Instant,
    },
    /// Already shown; waiting for the target key to be released.
    Fired,
    /// The user is using an ordinary shortcut; waiting for the target key to be released.
    Suppressed,
}

pub struct TriggerDetector {
    trigger: Trigger,
    state: State,
    modifiers: Modifiers,
}

impl TriggerDetector {
    pub fn new(trigger: Trigger) -> Self {
        Self {
            trigger,
            state: State::Idle,
            modifiers: Modifiers::NONE,
        }
    }

    pub fn on_event(&mut self, event: KeyEvent, now: Instant) -> Option<TriggerAction> {
        let previous = self.modifiers;
        if let KeyEvent::ModifiersChanged(m) = event {
            self.modifiers = m;
        }
        match self.trigger {
            Trigger::HoldModifier { key, .. } => self.hold_event(key, previous, event, now),
            Trigger::DoubleTap { key, interval } => {
                self.tap_event(key, interval, previous, event, now)
            }
            Trigger::Hotkey(_) | Trigger::External => None,
        }
    }

    pub fn on_tick(&mut self, now: Instant) -> Option<TriggerAction> {
        let deadline = self.next_deadline()?;
        if now < deadline {
            return None;
        }
        match self.state {
            State::Holding { .. } => {
                self.state = State::Fired;
                Some(TriggerAction::Show)
            }
            State::WaitSecond { .. } => {
                self.state = State::Idle;
                Some(TriggerAction::Cancel)
            }
            _ => None,
        }
    }

    pub fn trigger(&self) -> &Trigger {
        &self.trigger
    }

    /// The next time `on_tick` needs to be called.
    pub fn next_deadline(&self) -> Option<Instant> {
        match (self.state, &self.trigger) {
            (State::Holding { since }, Trigger::HoldModifier { delay, .. }) => Some(since + *delay),
            (State::WaitSecond { deadline }, _) => Some(deadline),
            _ => None,
        }
    }

    fn hold_event(
        &mut self,
        key: Modifiers,
        previous: Modifiers,
        event: KeyEvent,
        now: Instant,
    ) -> Option<TriggerAction> {
        use KeyEvent::*;
        match (self.state, event) {
            (State::Idle, ModifiersChanged(m)) if previous.is_empty() && m == key => {
                self.state = State::Holding { since: now };
                Some(TriggerAction::Prefetch)
            }
            (State::Holding { .. }, ModifiersChanged(m)) if m == key => None,
            (State::Holding { .. }, ModifiersChanged(m)) => {
                self.state = if m.contains(key) {
                    State::Suppressed
                } else {
                    State::Idle
                };
                Some(TriggerAction::Cancel)
            }
            (State::Holding { .. }, KeyDown | MouseDown) => {
                self.state = State::Suppressed;
                Some(TriggerAction::Cancel)
            }
            (State::Fired, ModifiersChanged(m)) if !m.contains(key) => {
                self.state = State::Idle;
                Some(TriggerAction::Released)
            }
            (State::Suppressed, ModifiersChanged(m)) if !m.contains(key) => {
                self.state = State::Idle;
                None
            }
            _ => None,
        }
    }

    fn tap_event(
        &mut self,
        key: Modifiers,
        interval: Duration,
        previous: Modifiers,
        event: KeyEvent,
        now: Instant,
    ) -> Option<TriggerAction> {
        use KeyEvent::*;
        match (self.state, event) {
            (State::Idle, ModifiersChanged(m)) if previous.is_empty() && m == key => {
                self.state = State::TapDown { since: now };
                None
            }
            (State::TapDown { since }, ModifiersChanged(m)) if m.is_empty() => {
                if now.duration_since(since) <= interval {
                    self.state = State::WaitSecond {
                        deadline: now + interval,
                    };
                    Some(TriggerAction::Prefetch)
                } else {
                    self.state = State::Idle;
                    None
                }
            }
            (State::TapDown { .. }, ModifiersChanged(m)) if m == key => None,
            (State::TapDown { .. }, _) => {
                self.state = State::Suppressed;
                None
            }
            (State::WaitSecond { deadline }, ModifiersChanged(m))
                if previous.is_empty() && m == key =>
            {
                if now <= deadline {
                    self.state = State::Fired;
                    Some(TriggerAction::Show)
                } else {
                    self.state = State::TapDown { since: now };
                    Some(TriggerAction::Cancel)
                }
            }
            (State::WaitSecond { .. }, _) => {
                self.state = if self.modifiers.is_empty() {
                    State::Idle
                } else {
                    State::Suppressed
                };
                Some(TriggerAction::Cancel)
            }
            (State::Fired | State::Suppressed, ModifiersChanged(m)) if m.is_empty() => {
                self.state = State::Idle;
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use KeyEvent::*;
    use TriggerAction::*;

    const CMD: Modifiers = Modifiers::META;
    const NONE: Modifiers = Modifiers::NONE;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn hold() -> TriggerDetector {
        TriggerDetector::new(Trigger::HoldModifier {
            key: CMD,
            delay: Trigger::DEFAULT_HOLD_DELAY,
        })
    }

    fn tap() -> TriggerDetector {
        TriggerDetector::new(Trigger::DoubleTap {
            key: CMD,
            interval: ms(350),
        })
    }

    #[test]
    fn holding_command_prefetches_then_shows_after_delay() {
        let t0 = Instant::now();
        let mut d = hold();
        assert_eq!(d.on_event(ModifiersChanged(CMD), t0), Some(Prefetch));
        assert_eq!(d.next_deadline(), Some(t0 + ms(800)));
        assert_eq!(d.on_tick(t0 + ms(799)), None);
        assert_eq!(d.on_tick(t0 + ms(800)), Some(Show));
        assert_eq!(d.next_deadline(), None);
    }

    #[test]
    fn releasing_early_cancels() {
        let t0 = Instant::now();
        let mut d = hold();
        d.on_event(ModifiersChanged(CMD), t0);
        assert_eq!(
            d.on_event(ModifiersChanged(NONE), t0 + ms(300)),
            Some(Cancel)
        );
        assert_eq!(d.on_tick(t0 + ms(900)), None);
    }

    #[test]
    fn using_a_normal_shortcut_cancels_until_command_is_released() {
        let t0 = Instant::now();
        let mut d = hold();
        d.on_event(ModifiersChanged(CMD), t0);
        assert_eq!(d.on_event(KeyDown, t0 + ms(100)), Some(Cancel));
        assert_eq!(d.on_tick(t0 + ms(2000)), None);
        assert_eq!(d.next_deadline(), None);
        assert_eq!(d.on_event(ModifiersChanged(NONE), t0 + ms(2100)), None);
        assert_eq!(
            d.on_event(ModifiersChanged(CMD), t0 + ms(2200)),
            Some(Prefetch)
        );
        assert_eq!(d.on_tick(t0 + ms(3000)), Some(Show));
    }

    #[test]
    fn mouse_click_and_extra_modifier_cancel() {
        let t0 = Instant::now();
        let mut d = hold();
        d.on_event(ModifiersChanged(CMD), t0);
        assert_eq!(d.on_event(MouseDown, t0 + ms(10)), Some(Cancel));

        let mut d = hold();
        d.on_event(ModifiersChanged(CMD), t0);
        assert_eq!(
            d.on_event(ModifiersChanged(CMD | Modifiers::SHIFT), t0 + ms(10)),
            Some(Cancel)
        );
        assert_eq!(d.on_event(ModifiersChanged(CMD), t0 + ms(20)), None);
        assert_eq!(d.on_tick(t0 + ms(2000)), None);
    }

    #[test]
    fn command_added_on_top_of_another_modifier_does_not_start() {
        let t0 = Instant::now();
        let mut d = hold();
        assert_eq!(d.on_event(ModifiersChanged(Modifiers::SHIFT), t0), None);
        assert_eq!(
            d.on_event(ModifiersChanged(Modifiers::SHIFT | CMD), t0 + ms(10)),
            None
        );
        assert_eq!(d.on_event(ModifiersChanged(CMD), t0 + ms(20)), None);
        assert_eq!(d.on_tick(t0 + ms(2000)), None);
    }

    #[test]
    fn after_show_waits_for_release_before_retriggering() {
        let t0 = Instant::now();
        let mut d = hold();
        d.on_event(ModifiersChanged(CMD), t0);
        d.on_tick(t0 + ms(800));
        assert_eq!(d.on_event(KeyDown, t0 + ms(900)), None);
        assert_eq!(
            d.on_event(ModifiersChanged(NONE), t0 + ms(1000)),
            Some(Released)
        );
        assert_eq!(
            d.on_event(ModifiersChanged(CMD), t0 + ms(1100)),
            Some(Prefetch)
        );
    }

    #[test]
    fn double_tap_does_not_report_the_release() {
        let t0 = Instant::now();
        let mut d = tap();
        d.on_event(ModifiersChanged(CMD), t0);
        d.on_event(ModifiersChanged(NONE), t0 + ms(80));
        assert_eq!(d.on_event(ModifiersChanged(CMD), t0 + ms(200)), Some(Show));
        assert_eq!(d.on_event(ModifiersChanged(NONE), t0 + ms(280)), None);
    }

    #[test]
    fn double_tap_within_interval_shows() {
        let t0 = Instant::now();
        let mut d = tap();
        assert_eq!(d.on_event(ModifiersChanged(CMD), t0), None);
        assert_eq!(
            d.on_event(ModifiersChanged(NONE), t0 + ms(80)),
            Some(Prefetch)
        );
        assert_eq!(d.next_deadline(), Some(t0 + ms(430)));
        assert_eq!(d.on_event(ModifiersChanged(CMD), t0 + ms(200)), Some(Show));
        assert_eq!(d.on_event(ModifiersChanged(NONE), t0 + ms(260)), None);
    }

    #[test]
    fn second_tap_too_late_cancels() {
        let t0 = Instant::now();
        let mut d = tap();
        d.on_event(ModifiersChanged(CMD), t0);
        d.on_event(ModifiersChanged(NONE), t0 + ms(80));
        assert_eq!(d.on_tick(t0 + ms(430)), Some(Cancel));
        assert_eq!(d.on_event(ModifiersChanged(CMD), t0 + ms(500)), None);
    }

    #[test]
    fn long_press_is_not_a_tap_and_typing_cancels() {
        let t0 = Instant::now();
        let mut d = tap();
        d.on_event(ModifiersChanged(CMD), t0);
        assert_eq!(d.on_event(ModifiersChanged(NONE), t0 + ms(600)), None);

        let mut d = tap();
        d.on_event(ModifiersChanged(CMD), t0);
        d.on_event(ModifiersChanged(NONE), t0 + ms(80));
        assert_eq!(d.on_event(KeyDown, t0 + ms(120)), Some(Cancel));
        assert_eq!(d.on_event(ModifiersChanged(CMD), t0 + ms(150)), None);
    }

    #[test]
    fn shortcut_during_first_tap_is_ignored() {
        let t0 = Instant::now();
        let mut d = tap();
        d.on_event(ModifiersChanged(CMD), t0);
        assert_eq!(d.on_event(KeyDown, t0 + ms(30)), None);
        assert_eq!(d.on_event(ModifiersChanged(NONE), t0 + ms(60)), None);
        assert_eq!(d.next_deadline(), None);
    }

    #[test]
    fn hotkey_and_external_are_ignored() {
        let t0 = Instant::now();
        for trigger in [
            Trigger::External,
            Trigger::Hotkey(crate::keys::Chord::parse("Mod+K", crate::keys::Os::MacOS).unwrap()),
        ] {
            let mut d = TriggerDetector::new(trigger);
            assert_eq!(d.on_event(ModifiersChanged(CMD), t0), None);
            assert_eq!(d.on_tick(t0 + ms(5000)), None);
        }
    }

    #[test]
    fn detector_reports_its_trigger() {
        let trigger = Trigger::DoubleTap {
            key: Modifiers::ALT,
            interval: Trigger::DEFAULT_DOUBLE_TAP_INTERVAL,
        };
        assert_eq!(TriggerDetector::new(trigger.clone()).trigger(), &trigger);
    }
}
