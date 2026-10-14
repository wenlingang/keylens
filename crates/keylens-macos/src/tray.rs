//! Status bar item. Can only be created and used on the main thread.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::NSString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Permission,
    /// Create or open the user data file for the frontmost app.
    EditCurrentApp,
    OpenDataFolder,
    Quit,
}

impl TrayCommand {
    const ALL: [TrayCommand; 4] = [
        TrayCommand::Permission,
        TrayCommand::EditCurrentApp,
        TrayCommand::OpenDataFolder,
        TrayCommand::Quit,
    ];

    fn tag(self) -> isize {
        self as isize + 1
    }

    fn from_tag(tag: isize) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.tag() == tag)
    }
}

pub struct TrayLabels {
    /// Disabled first menu item, e.g. "Keylens 0.1.0".
    pub version: String,
    pub permission_ok: String,
    pub permission_missing: String,
    pub edit_current: String,
    pub open_data: String,
    pub quit: String,
}

pub struct TargetIvars {
    on_command: Box<dyn Fn(TrayCommand)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeylensTrayTarget"]
    #[ivars = TargetIvars]
    struct Target;

    impl Target {
        #[unsafe(method(menuItemClicked:))]
        fn menu_item_clicked(&self, sender: &NSMenuItem) {
            if let Some(command) = TrayCommand::from_tag(sender.tag()) {
                (self.ivars().on_command)(command);
            }
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, on_command: Box<dyn Fn(TrayCommand)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars { on_command });
        // SAFETY: Calls NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

/// The status bar item and its menu. Must be held for as long as it is needed: after drop the icon disappears and clicks do nothing (menu items' target is a weak reference).
pub struct Tray {
    item: Retained<NSStatusItem>,
    permission_item: Retained<NSMenuItem>,
    errors_item: Retained<NSMenuItem>,
    labels: TrayLabels,
    mtm: MainThreadMarker,
    _menu: Retained<NSMenu>,
    _target: Retained<Target>,
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    target: &Target,
    command: Option<TrayCommand>,
) -> Retained<NSMenuItem> {
    let action = command.map(|_| sel!(menuItemClicked:));
    // SAFETY: Standard NSMenuItem initialization.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(""),
        )
    };
    if let Some(command) = command {
        item.setTag(command.tag());
        // SAFETY: target is held by Tray, whose lifetime covers the menu items.
        unsafe { item.setTarget(Some(&**target as &AnyObject)) };
    }
    item
}

impl Tray {
    pub fn install(
        mtm: MainThreadMarker,
        labels: TrayLabels,
        on_command: Box<dyn Fn(TrayCommand)>,
    ) -> Tray {
        let target = Target::new(mtm, on_command);
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let menu = NSMenu::new(mtm);
        menu.addItem(&menu_item(mtm, &labels.version, &target, None));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let permission_item = menu_item(
            mtm,
            &labels.permission_ok,
            &target,
            Some(TrayCommand::Permission),
        );
        menu.addItem(&permission_item);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        // Shown only while data files have errors (see `set_data_errors`).
        let errors_item = menu_item(mtm, "", &target, None);
        errors_item.setHidden(true);
        menu.addItem(&errors_item);
        menu.addItem(&menu_item(
            mtm,
            &labels.edit_current,
            &target,
            Some(TrayCommand::EditCurrentApp),
        ));
        menu.addItem(&menu_item(
            mtm,
            &labels.open_data,
            &target,
            Some(TrayCommand::OpenDataFolder),
        ));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&menu_item(
            mtm,
            &labels.quit,
            &target,
            Some(TrayCommand::Quit),
        ));
        item.setMenu(Some(&menu));
        let tray = Tray {
            item,
            permission_item,
            errors_item,
            labels,
            mtm,
            _menu: menu,
            _target: target,
        };
        tray.set_permission(true);
        tray
    }

    /// Updates the permission status: the icon title and the first menu item.
    /// Shows `text` (e.g. "2 data files have errors") in the menu, or hides the item for `None`.
    pub fn set_data_errors(&self, text: Option<&str>) {
        match text {
            Some(text) => {
                self.errors_item.setTitle(&NSString::from_str(text));
                self.errors_item.setHidden(false);
            }
            None => self.errors_item.setHidden(true),
        }
    }

    pub fn set_permission(&self, granted: bool) {
        if let Some(button) = self.item.button(self.mtm) {
            button.setTitle(&NSString::from_str(if granted { "⌘" } else { "⌘!" }));
        }
        let title = if granted {
            &self.labels.permission_ok
        } else {
            &self.labels.permission_missing
        };
        self.permission_item.setTitle(&NSString::from_str(title));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_round_trips_through_its_tag() {
        let all = [
            TrayCommand::Permission,
            TrayCommand::EditCurrentApp,
            TrayCommand::OpenDataFolder,
            TrayCommand::Quit,
        ];
        for command in all {
            assert_eq!(TrayCommand::from_tag(command.tag()), Some(command));
        }
        assert_eq!(TrayCommand::from_tag(0), None);
    }
}
