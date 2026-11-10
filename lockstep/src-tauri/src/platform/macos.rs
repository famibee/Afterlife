use std::ffi::c_void;

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowLayer, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly, kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID,
};

use super::WindowInfo;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementSetAttributeValue(
        element: *const c_void,
        attribute: CFStringRef,
        value: *const c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

fn value(dict: &CFDictionary, key: CFStringRef) -> Option<CFType> {
    let v = dict.find(key as *const c_void)?;
    Some(unsafe { CFType::wrap_under_get_rule(*v as _) })
}

fn number(dict: &CFDictionary, key: CFStringRef) -> Option<i64> {
    value(dict, key)?.downcast::<CFNumber>()?.to_i64()
}

/// kCGWindowBounds は X/Y/Width/Height を持つ入れ子の辞書
fn bounds(dict: &CFDictionary) -> Option<(i32, i32, i32, i32)> {
    let key = CFString::new("kCGWindowBounds");
    let sub = value(dict, key.as_concrete_TypeRef())?.downcast::<CFDictionary>()?;
    let get = |name: &str| -> Option<i32> {
        let k = CFString::new(name);
        let n = value(&sub, k.as_concrete_TypeRef())?.downcast::<CFNumber>()?;
        Some(n.to_f64()? as i32)
    };
    Some((get("X")?, get("Y")?, get("Width")?, get("Height")?))
}

/// 画面上の通常ウィンドウ（レイヤー0）を前面から順に渡す。f が false を返すと打ち切る。
fn each_window<F: FnMut(&CFDictionary) -> bool>(mut f: F) {
    let Some(array) = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return;
    };
    let array: CFArray<CFDictionary> =
        unsafe { CFArray::wrap_under_get_rule(array.as_concrete_TypeRef() as _) };
    for dict in array.iter() {
        if number(&dict, unsafe { kCGWindowLayer }).unwrap_or(0) != 0 {
            continue;
        }
        if !f(&dict) {
            break;
        }
    }
}

fn owner_pid(dict: &CFDictionary) -> i32 {
    number(dict, unsafe { kCGWindowOwnerPID }).unwrap_or(0) as i32
}

pub fn list_windows() -> Vec<WindowInfo> {
    let own_pid = std::process::id() as i32;
    let mut result = Vec::new();
    each_window(|dict| {
        let pid = owner_pid(dict);
        if let Some((x, y, width, height)) = bounds(dict)
            && width >= 100
            && height >= 60
            && pid != own_pid
        {
            let app_name = value(dict, unsafe { kCGWindowOwnerName })
                .and_then(|v| v.downcast::<CFString>())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let id = number(dict, unsafe { kCGWindowNumber }).unwrap_or(0) as u64;
            result.push(WindowInfo { id, pid, app_name, x, y, width, height });
        }
        true
    });
    result
}

/// タイトルバーを含むウィンドウ全体の枠を返す（Windows版はクライアント領域のみ）。
pub fn window_rect(id: u64) -> Option<(i32, i32, i32, i32)> {
    let mut found = None;
    each_window(|dict| {
        if number(dict, unsafe { kCGWindowNumber }) == Some(id as i64) {
            found = bounds(dict);
            return false;
        }
        true
    });
    found
}

/// 最前面の通常ウィンドウの持ち主で判定する。ただし、ウィンドウを持たない
/// アプリ（デスクトップだけのFinder等）が実際にはアクティブで、対象のウィンドウが
/// たまたまウィンドウ一覧の先頭に残っているだけのケースを誤検出しないよう、
/// NSWorkspace が報告する実際のアクティブアプリの一致も併せて確かめる。
pub fn is_frontmost(_id: u64, pid: i32) -> bool {
    use objc2_app_kit::NSWorkspace;
    let app_is_active = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|a| a.processIdentifier() as i32 == pid)
        .unwrap_or(false);
    if !app_is_active {
        return false;
    }
    let mut front = None;
    each_window(|dict| {
        front = Some(owner_pid(dict));
        false
    });
    front == Some(pid)
}

/// アプリ単位で前面化する（macOSではウィンドウ単位の指定はしない）。
pub fn focus_window(_id: u64, pid: i32) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
    }
    // macOS 14以降は背面アプリからの activate が無視されうるため、Accessibility経由でも要求する
    unsafe {
        let element = AXUIElementCreateApplication(pid);
        if !element.is_null() {
            let attr = CFString::new("AXFrontmost");
            AXUIElementSetAttributeValue(
                element,
                attr.as_concrete_TypeRef(),
                CFBoolean::true_value().as_CFTypeRef(),
            );
            CFRelease(element);
        }
    }
}

pub fn check_input_permission() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn open_permission_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

