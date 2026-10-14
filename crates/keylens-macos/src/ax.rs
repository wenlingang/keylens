//! AXUIElement 的安全封装。所有 unsafe 调用都集中在这里。

use std::ptr::NonNull;

use keylens_core::SourceError;
use keylens_platform::PlatformError;
use objc2_application_services::{
    AXCopyMultipleAttributeOptions, AXError, AXIsProcessTrusted, AXIsProcessTrustedWithOptions,
    AXUIElement, kAXTrustedCheckOptionPrompt,
};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType,
};

use crate::menu_tree::{ItemInfo, MenuNode};

/// AX 消息超时（spec §3.4）。
const MESSAGING_TIMEOUT_SECS: f32 = 1.0;

/// 批量读取的属性，顺序与 `ItemInfo` 的字段对应。
const ITEM_ATTRIBUTES: [&str; 6] = [
    "AXRole",
    "AXTitle",
    "AXEnabled",
    "AXMenuItemCmdChar",
    "AXMenuItemCmdVirtualKey",
    "AXMenuItemCmdModifiers",
];

/// 一个 AX 元素。
#[derive(Clone)]
pub struct AxNode(CFRetained<AXUIElement>);

// SAFETY: AXUIElement 是对其他进程中元素的不可变引用，Accessibility API 可以在任意线程调用。
unsafe impl Send for AxNode {}
// SAFETY: 同上，元素本身没有可变状态。
unsafe impl Sync for AxNode {}

fn source_error(err: AXError) -> SourceError {
    if err == AXError::APIDisabled {
        SourceError::PermissionDenied
    } else if err == AXError::CannotComplete {
        SourceError::Timeout
    } else {
        SourceError::Other(format!("accessibility error {}", err.0))
    }
}

fn attribute(element: &AXUIElement, name: &str) -> Result<CFRetained<CFType>, AXError> {
    let name = CFString::from_str(name);
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: `value` 是有效的输出指针；成功时它持有一个 +1 引用，由 CFRetained 接管。
    let err = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut value)) };
    if err != AXError::Success {
        return Err(err);
    }
    NonNull::new(value as *mut CFType)
        // SAFETY: Copy 规则返回的对象已经 retain 过一次。
        .map(|p| unsafe { CFRetained::from_raw(p) })
        .ok_or(AXError::NoValue)
}

fn string(value: Option<&CFRetained<CFType>>) -> Option<String> {
    value?.downcast_ref::<CFString>().map(|s| s.to_string())
}

fn number(value: Option<&CFRetained<CFType>>) -> Option<i64> {
    value?.downcast_ref::<CFNumber>()?.as_i64()
}

fn boolean(value: Option<&CFRetained<CFType>>) -> Option<bool> {
    value?.downcast_ref::<CFBoolean>().map(|b| b.as_bool())
}

fn elements(value: CFRetained<CFType>) -> Vec<AxNode> {
    let Ok(array) = value.downcast::<CFArray>() else {
        return Vec::new();
    };
    // SAFETY: AXChildren / AXWindows 的值是 AXUIElement 数组。
    unsafe { array.cast_unchecked::<AXUIElement>() }
        .to_vec()
        .into_iter()
        .map(AxNode)
        .collect()
}

impl MenuNode for AxNode {
    fn info(&self) -> ItemInfo {
        let names: Vec<CFRetained<CFString>> = ITEM_ATTRIBUTES
            .iter()
            .map(|a| CFString::from_str(a))
            .collect();
        let names = CFArray::from_retained_objects(&names);
        let mut out: *const CFArray = std::ptr::null();
        // SAFETY: `names` 是 CFString 数组；`out` 是有效的输出指针，成功时持有 +1 引用。
        let err = unsafe {
            self.0.copy_multiple_attribute_values(
                names.as_ref(),
                AXCopyMultipleAttributeOptions::empty(),
                NonNull::from(&mut out),
            )
        };
        let Some(ptr) = NonNull::new(out as *mut CFArray).filter(|_| err == AXError::Success)
        else {
            return ItemInfo::default();
        };
        // SAFETY: Copy 规则返回的数组已经 retain 过一次。
        let values = unsafe { CFRetained::from_raw(ptr) };
        // SAFETY: 数组元素都是 CF 对象；读取失败的属性是 AXValue 错误占位，下面的 downcast 会得到 None。
        let values: Vec<CFRetained<CFType>> = unsafe { values.cast_unchecked::<CFType>() }.to_vec();
        let get = |i: usize| values.get(i);
        ItemInfo {
            role: string(get(0)).unwrap_or_default(),
            title: string(get(1)).unwrap_or_default(),
            enabled: boolean(get(2)).unwrap_or(true),
            cmd_char: string(get(3)).filter(|c| !c.is_empty()),
            cmd_vkey: number(get(4)),
            cmd_modifiers: number(get(5)),
        }
    }

    fn children(&self) -> Vec<AxNode> {
        attribute(&self.0, "AXChildren")
            .map(elements)
            .unwrap_or_default()
    }
}

fn application(pid: i32) -> CFRetained<AXUIElement> {
    // SAFETY: 为任意 pid 创建元素都是安全的；pid 无效时后续调用返回错误。
    let app = unsafe { AXUIElement::new_application(pid) };
    // SAFETY: 设置超时只影响这个元素后续的消息。
    unsafe { app.set_messaging_timeout(MESSAGING_TIMEOUT_SECS) };
    app
}

pub fn menu_bar(pid: i32) -> Result<AxNode, SourceError> {
    let value = attribute(&application(pid), "AXMenuBar").map_err(source_error)?;
    value
        .downcast::<AXUIElement>()
        .map(AxNode)
        .map_err(|_| SourceError::Other("AXMenuBar is not an element".into()))
}

pub fn title(node: &AxNode) -> Option<String> {
    attribute(&node.0, "AXTitle")
        .ok()?
        .downcast::<CFString>()
        .ok()
        .map(|s| s.to_string())
}

pub fn press(node: &AxNode) -> Result<(), PlatformError> {
    let action = CFString::from_str("AXPress");
    // SAFETY: 对菜单项元素执行 AXPress 动作。
    let err = unsafe { node.0.perform_action(&action) };
    match err {
        e if e == AXError::Success => Ok(()),
        e if e == AXError::APIDisabled => Err(PlatformError::PermissionDenied),
        e if e == AXError::InvalidUIElement => Err(PlatformError::Stale),
        e => Err(PlatformError::Other(format!("AXPress failed: {}", e.0))),
    }
}

/// 调试用：列出应用的窗口标题。
pub fn window_titles(pid: i32) -> Vec<String> {
    attribute(&application(pid), "AXWindows")
        .map(elements)
        .unwrap_or_default()
        .iter()
        .map(|w| title(w).unwrap_or_default())
        .collect()
}

pub fn is_trusted() -> bool {
    // SAFETY: 无参数的查询函数。
    unsafe { AXIsProcessTrusted() }
}

/// 请求权限：系统会弹出提示并把本程序加入「辅助功能」列表（开关默认关闭）。
pub fn request_trust() {
    // SAFETY: kAXTrustedCheckOptionPrompt 是系统提供的常量字符串。
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: options 是 CFString → CFBoolean 的字典，符合 API 要求。
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_ref())) };
}
