//! AXUIElement 的安全封装。所有 unsafe 调用都集中在这里。

use std::ptr::NonNull;
use std::sync::Once;

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

/// 读取子元素失败时：没有子元素属于正常情况，超时和无权限要中止遍历。
fn children_outcome(err: AXError) -> Result<(), SourceError> {
    if err == AXError::NoValue || err == AXError::AttributeUnsupported {
        Ok(())
    } else {
        Err(source_error(err))
    }
}

/// AXPress 的返回值。打开模态窗口（打开面板、提示框）的动作会返回 CannotComplete，
/// 但动作已经派发（见 AXUIElement.h），调用方不应重试，否则会重复执行。
fn press_outcome(err: AXError) -> Result<(), PlatformError> {
    if err == AXError::Success || err == AXError::CannotComplete {
        Ok(())
    } else if err == AXError::APIDisabled {
        Err(PlatformError::PermissionDenied)
    } else if err == AXError::InvalidUIElement {
        Err(PlatformError::Stale)
    } else {
        Err(PlatformError::Other(format!("AXPress failed: {}", err.0)))
    }
}

/// 设置全局默认的 AX 消息超时。对单个元素设置的超时不会传给它的子元素
/// （AXUIElement.h：只对该对象生效），所以要通过系统级元素设置全局默认值。
fn ensure_global_timeout() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: 系统级元素总是可以创建；对它设置超时会改变本进程所有 AX 调用的默认超时。
        let system = unsafe { AXUIElement::new_system_wide() };
        // SAFETY: 同上。
        unsafe { system.set_messaging_timeout(MESSAGING_TIMEOUT_SECS) };
    });
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
    fn info(&self) -> Result<ItemInfo, SourceError> {
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
        if err != AXError::Success {
            return Err(source_error(err));
        }
        let ptr = NonNull::new(out as *mut CFArray)
            .ok_or_else(|| SourceError::Other("empty attribute values".into()))?;
        // SAFETY: Copy 规则返回的数组已经 retain 过一次。
        let values = unsafe { CFRetained::from_raw(ptr) };
        // SAFETY: 数组元素都是 CF 对象；读取失败的属性是 AXValue 错误占位，下面的 downcast 会得到 None。
        let values: Vec<CFRetained<CFType>> = unsafe { values.cast_unchecked::<CFType>() }.to_vec();
        let get = |i: usize| values.get(i);
        Ok(ItemInfo {
            role: string(get(0)).unwrap_or_default(),
            title: string(get(1)).unwrap_or_default(),
            enabled: boolean(get(2)).unwrap_or(true),
            cmd_char: string(get(3)).filter(|c| !c.is_empty()),
            cmd_vkey: number(get(4)),
            cmd_modifiers: number(get(5)),
        })
    }

    fn children(&self) -> Result<Vec<AxNode>, SourceError> {
        match attribute(&self.0, "AXChildren") {
            Ok(value) => Ok(elements(value)),
            Err(err) => children_outcome(err).map(|()| Vec::new()),
        }
    }
}

fn application(pid: i32) -> CFRetained<AXUIElement> {
    ensure_global_timeout();
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
    press_outcome(err)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_that_opens_a_modal_counts_as_dispatched() {
        // 打开模态窗口的动作会返回 CannotComplete，但动作已经执行（AXUIElement.h）。
        assert!(press_outcome(AXError::CannotComplete).is_ok());
        assert!(press_outcome(AXError::Success).is_ok());
        assert!(matches!(
            press_outcome(AXError::APIDisabled),
            Err(PlatformError::PermissionDenied)
        ));
        assert!(matches!(
            press_outcome(AXError::InvalidUIElement),
            Err(PlatformError::Stale)
        ));
        assert!(matches!(
            press_outcome(AXError::Failure),
            Err(PlatformError::Other(_))
        ));
    }

    #[test]
    fn missing_children_are_empty_but_timeouts_are_errors() {
        assert_eq!(children_outcome(AXError::NoValue), Ok(()));
        assert_eq!(children_outcome(AXError::AttributeUnsupported), Ok(()));
        assert_eq!(
            children_outcome(AXError::CannotComplete),
            Err(SourceError::Timeout)
        );
        assert_eq!(
            children_outcome(AXError::APIDisabled),
            Err(SourceError::PermissionDenied)
        );
    }
}
