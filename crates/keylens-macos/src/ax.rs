//! Safe wrapper around AXUIElement. All unsafe calls are concentrated here.

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

/// AX messaging timeout (spec §3.4).
const MESSAGING_TIMEOUT_SECS: f32 = 1.0;

/// Attributes read in a batch, in the same order as the fields of `ItemInfo`.
const ITEM_ATTRIBUTES: [&str; 6] = [
    "AXRole",
    "AXTitle",
    "AXEnabled",
    "AXMenuItemCmdChar",
    "AXMenuItemCmdVirtualKey",
    "AXMenuItemCmdModifiers",
];

/// An AX element.
#[derive(Clone)]
pub struct AxNode(CFRetained<AXUIElement>);

// SAFETY: AXUIElement is an immutable reference to an element in another process; the Accessibility API can be called from any thread.
unsafe impl Send for AxNode {}
// SAFETY: Same as above; the element itself has no mutable state.
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

/// When reading children fails: having no children is normal, while timeouts and missing permission must abort the traversal.
fn children_outcome(err: AXError) -> Result<(), SourceError> {
    if err == AXError::NoValue || err == AXError::AttributeUnsupported {
        Ok(())
    } else {
        Err(source_error(err))
    }
}

/// Return value of AXPress. Actions that open a modal window (open panel, alert) return CannotComplete,
/// but the action has already been dispatched (see AXUIElement.h); callers should not retry, or it would run twice.
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

/// Sets the global default AX messaging timeout. A timeout set on a single element is not passed on to its children
/// (AXUIElement.h: it only applies to that object), so the global default is set through the system-wide element.
fn ensure_global_timeout() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: The system-wide element can always be created; setting a timeout on it changes the default timeout of all AX calls in this process.
        let system = unsafe { AXUIElement::new_system_wide() };
        // SAFETY: Same as above.
        unsafe { system.set_messaging_timeout(MESSAGING_TIMEOUT_SECS) };
    });
}

fn attribute(element: &AXUIElement, name: &str) -> Result<CFRetained<CFType>, AXError> {
    let name = CFString::from_str(name);
    let mut value: *const CFType = std::ptr::null();
    // SAFETY: `value` is a valid output pointer; on success it holds a +1 reference, taken over by CFRetained.
    let err = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut value)) };
    if err != AXError::Success {
        return Err(err);
    }
    NonNull::new(value as *mut CFType)
        // SAFETY: The object returned under the Copy rule has already been retained once.
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
    // SAFETY: The values of AXChildren / AXWindows are arrays of AXUIElement.
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
        // SAFETY: `names` is a CFString array; `out` is a valid output pointer and holds a +1 reference on success.
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
        // SAFETY: The array returned under the Copy rule has already been retained once.
        let values = unsafe { CFRetained::from_raw(ptr) };
        // SAFETY: Array elements are all CF objects; attributes that failed to read are AXValue error placeholders, and the downcast below yields None.
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
    // SAFETY: Creating an element for any pid is safe; later calls return an error if the pid is invalid.
    let app = unsafe { AXUIElement::new_application(pid) };
    // SAFETY: Setting the timeout only affects later messages to this element.
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
    // SAFETY: Performs the AXPress action on a menu item element.
    let err = unsafe { node.0.perform_action(&action) };
    press_outcome(err)
}

/// For debugging: lists an app's window titles.
pub fn window_titles(pid: i32) -> Vec<String> {
    attribute(&application(pid), "AXWindows")
        .map(elements)
        .unwrap_or_default()
        .iter()
        .map(|w| title(w).unwrap_or_default())
        .collect()
}

pub fn is_trusted() -> bool {
    // SAFETY: A query function with no arguments.
    unsafe { AXIsProcessTrusted() }
}

/// Requests permission: the system shows a prompt and adds this program to the "Accessibility" list (toggle off by default).
pub fn request_trust() {
    // SAFETY: kAXTrustedCheckOptionPrompt is a constant string provided by the system.
    let key: &CFString = unsafe { kAXTrustedCheckOptionPrompt };
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(true)]);
    // SAFETY: options is a CFString -> CFBoolean dictionary, as the API requires.
    unsafe { AXIsProcessTrustedWithOptions(Some(options.as_ref())) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_that_opens_a_modal_counts_as_dispatched() {
        // Actions that open a modal window return CannotComplete, but the action has already run (AXUIElement.h).
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
