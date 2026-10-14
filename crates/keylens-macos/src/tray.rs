//! 菜单栏图标。只能在主线程创建和使用。

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::NSString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Permission,
    OpenDataFolder,
    Quit,
}

impl TrayCommand {
    const ALL: [TrayCommand; 3] = [
        TrayCommand::Permission,
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
    pub permission_ok: String,
    pub permission_missing: String,
    pub open_data: String,
    /// 数据文件有错误时显示的文字，没有错误时为 `None`。
    pub data_errors: Option<String>,
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
        // SAFETY: 调用 NSObject 的 init。
        unsafe { msg_send![super(this), init] }
    }
}

/// 菜单栏图标和它的菜单。必须一直持有，drop 后图标消失、点击无效（菜单项的 target 是弱引用）。
pub struct Tray {
    item: Retained<NSStatusItem>,
    permission_item: Retained<NSMenuItem>,
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
    // SAFETY: 标准的 NSMenuItem 初始化。
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
        // SAFETY: target 由 Tray 持有，生命周期覆盖菜单项。
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
        let permission_item = menu_item(
            mtm,
            &labels.permission_ok,
            &target,
            Some(TrayCommand::Permission),
        );
        menu.addItem(&permission_item);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        if let Some(errors) = &labels.data_errors {
            menu.addItem(&menu_item(mtm, errors, &target, None));
        }
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
            labels,
            mtm,
            _menu: menu,
            _target: target,
        };
        tray.set_permission(true);
        tray
    }

    /// 更新权限状态：图标标题和第一个菜单项。
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
