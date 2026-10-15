//! Status bar item. Can only be created and used on the main thread.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::NSString;

pub use keylens_platform::tray::{TrayCommand, TrayHandle, TrayLabels};

fn tag(command: TrayCommand) -> isize {
    command as isize + 1
}

fn from_tag(tag: isize) -> Option<TrayCommand> {
    TrayCommand::ALL.into_iter().find(|c| self::tag(*c) == tag)
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
            if let Some(command) = from_tag(sender.tag()) {
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
    version_item: Retained<NSMenuItem>,
    edit_item: Retained<NSMenuItem>,
    open_item: Retained<NSMenuItem>,
    settings_item: Retained<NSMenuItem>,
    quit_item: Retained<NSMenuItem>,
    labels: RefCell<TrayLabels>,
    granted: Cell<bool>,
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
        item.setTag(tag(command));
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
        let version_item = menu_item(mtm, &labels.version, &target, None);
        menu.addItem(&version_item);
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
        let edit_item = menu_item(
            mtm,
            &labels.edit_current,
            &target,
            Some(TrayCommand::EditCurrentApp),
        );
        menu.addItem(&edit_item);
        let open_item = menu_item(
            mtm,
            &labels.open_data,
            &target,
            Some(TrayCommand::OpenDataFolder),
        );
        menu.addItem(&open_item);
        let settings_item = menu_item(mtm, &labels.settings, &target, Some(TrayCommand::Settings));
        menu.addItem(&settings_item);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let quit_item = menu_item(mtm, &labels.quit, &target, Some(TrayCommand::Quit));
        menu.addItem(&quit_item);
        item.setMenu(Some(&menu));
        let tray = Tray {
            item,
            permission_item,
            errors_item,
            version_item,
            edit_item,
            open_item,
            settings_item,
            quit_item,
            labels: RefCell::new(labels),
            granted: Cell::new(true),
            mtm,
            _menu: menu,
            _target: target,
        };
        tray.set_permission(true);
        tray
    }
}

impl TrayHandle for Tray {
    fn set_data_errors(&self, text: Option<&str>) {
        match text {
            Some(text) => {
                self.errors_item.setTitle(&NSString::from_str(text));
                self.errors_item.setHidden(false);
            }
            None => self.errors_item.setHidden(true),
        }
    }

    fn set_permission(&self, granted: bool) {
        self.granted.set(granted);
        if let Some(button) = self.item.button(self.mtm) {
            button.setTitle(&NSString::from_str(if granted { "⌘" } else { "⌘!" }));
        }
        let labels = self.labels.borrow();
        let title = if granted {
            &labels.permission_ok
        } else {
            &labels.permission_missing
        };
        self.permission_item.setTitle(&NSString::from_str(title));
    }

    fn set_labels(&self, labels: TrayLabels) {
        let title = |item: &NSMenuItem, text: &str| item.setTitle(&NSString::from_str(text));
        title(&self.version_item, &labels.version);
        title(&self.edit_item, &labels.edit_current);
        title(&self.open_item, &labels.open_data);
        title(&self.settings_item, &labels.settings);
        title(&self.quit_item, &labels.quit);
        *self.labels.borrow_mut() = labels;
        self.set_permission(self.granted.get());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_round_trips_through_its_tag() {
        for command in TrayCommand::ALL {
            assert_eq!(from_tag(tag(command)), Some(command));
        }
        assert_eq!(from_tag(0), None);
    }
}
