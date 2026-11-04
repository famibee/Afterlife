use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::window::{
    copy_window_info, create_image, kCGNullWindowID, kCGWindowImageBoundsIgnoreFraming,
    kCGWindowLayer, kCGWindowListExcludeDesktopElements, kCGWindowListOptionIncludingWindow,
    kCGWindowListOptionOnScreenOnly, kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID,
};

use super::WindowInfo;

/// `kCGWindowBounds` の辞書表現（CGRectMakeWithDictionaryRepresentation準拠）から矩形を取り出す。
fn parse_bounds(dict: &CFDictionary) -> Option<(i32, i32, i32, i32)> {
    // kCGWindowBoundsの値はウィンドウ辞書のトップレベルではなく、X/Y/Width/Heightを
    // 持つ入れ子の辞書なので、まずそのサブ辞書を取り出す必要がある。
    let bounds_key = CFString::new("kCGWindowBounds");
    let bounds_value = dict.find(bounds_key.as_concrete_TypeRef() as *const _)?;
    let bounds_dict = unsafe { core_foundation::base::CFType::wrap_under_get_rule(*bounds_value as _) }
        .downcast::<CFDictionary>()?;
    let get_num = |key: &str| -> Option<f64> {
        let cf_key = CFString::new(key);
        let value = bounds_dict.find(cf_key.as_concrete_TypeRef() as *const _)?;
        let ty = unsafe { core_foundation::base::CFType::wrap_under_get_rule(*value as _) };
        ty.downcast::<CFNumber>()?.to_f64()
    };
    Some((
        get_num("X")? as i32,
        get_num("Y")? as i32,
        get_num("Width")? as i32,
        get_num("Height")? as i32,
    ))
}

fn each_window<F: FnMut(&CFDictionary)>(mut f: F) {
    let Some(array) = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return;
    };
    // CFArray<CFDictionary<CFString, CFType>> として要素を取り出す
    let array: CFArray<core_foundation::dictionary::CFDictionary> =
        unsafe { CFArray::wrap_under_get_rule(array.as_concrete_TypeRef() as _) };
    for item in array.iter() {
        f(&item);
    }
}

pub fn list_windows() -> Vec<WindowInfo> {
    let mut result = Vec::new();
    each_window(|dict| {
        let layer = dict
            .find(unsafe { kCGWindowLayer as *const _ })
            .and_then(|v| unsafe { core_foundation::base::CFType::wrap_under_get_rule(*v as _) }.downcast::<CFNumber>())
            .and_then(|n| n.to_i32())
            .unwrap_or(0);
        // レイヤー0は通常のアプリウィンドウ。メニューバー等の特殊レイヤーは除外する。
        if layer != 0 {
            return;
        }
        let Some((x, y, w, h)) = parse_bounds(dict) else {
            return;
        };
        if w < 100 || h < 60 {
            return; // 小さすぎるパネル等は候補から除外
        }
        let pid = dict
            .find(unsafe { kCGWindowOwnerPID as *const _ })
            .and_then(|v| unsafe { core_foundation::base::CFType::wrap_under_get_rule(*v as _) }.downcast::<CFNumber>())
            .and_then(|n| n.to_i32())
            .unwrap_or(0);
        if pid == std::process::id() as i32 {
            return; // Afterlife自身のウィンドウ（ハイライトオーバーレイ含む）は候補から除外
        }
        let id = dict
            .find(unsafe { kCGWindowNumber as *const _ })
            .and_then(|v| unsafe { core_foundation::base::CFType::wrap_under_get_rule(*v as _) }.downcast::<CFNumber>())
            .and_then(|n| n.to_i64())
            .unwrap_or(0);
        let app_name = dict
            .find(unsafe { kCGWindowOwnerName as *const _ })
            .and_then(|v| unsafe { core_foundation::base::CFType::wrap_under_get_rule(*v as _) }.downcast::<CFString>())
            .map(|s| s.to_string())
            .unwrap_or_default();

        result.push(WindowInfo {
            id: id as u64,
            pid,
            app_name,
            x,
            y,
            width: w,
            height: h,
        });
    });
    result
}

pub fn window_rect(id: u64) -> Option<(i32, i32, i32, i32)> {
    let mut found = None;
    each_window(|dict| {
        if found.is_some() {
            return;
        }
        let this_id = dict
            .find(unsafe { kCGWindowNumber as *const _ })
            .and_then(|v| unsafe { core_foundation::base::CFType::wrap_under_get_rule(*v as _) }.downcast::<CFNumber>())
            .and_then(|n| n.to_i64())
            .unwrap_or(-1);
        if this_id as u64 == id {
            found = parse_bounds(dict);
        }
    });
    found
}

pub fn activate_window(_id: u64, pid: i32) -> bool {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
        return false;
    };
    app.activateWithOptions(NSApplicationActivationOptions::empty())
}

/// 現在の前面アプリのpidを返す（復帰用）。
pub fn frontmost_window() -> Option<u64> {
    use objc2_app_kit::NSWorkspace;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    Some(app.processIdentifier() as u64)
}

pub fn restore_focus(handle: u64) -> bool {
    activate_window(0, handle as i32)
}

/// Accessibility権限（他アプリへのイベント合成に必要）。
pub fn check_accessibility_permission() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

pub fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

/// 指定ウィンドウの内容をRGBAで取得する（操作パッドのスナップショット表示用）。
/// Screen Recording権限が必要（未付与の場合は黒画像または取得失敗になりうる）。
pub fn capture_window_rgba(id: u64) -> Option<(Vec<u8>, u32, u32)> {
    // CGRectNull相当（origin無限大）を渡すと、kCGWindowListOptionIncludingWindowと
    // 組み合わせて指定ウィンドウの自然な範囲全体をキャプチャできる。
    let null_rect = CGRect::new(
        &CGPoint::new(f64::INFINITY, f64::INFINITY),
        &CGSize::new(0.0, 0.0),
    );
    let image = create_image(
        null_rect,
        kCGWindowListOptionIncludingWindow,
        id as u32,
        kCGWindowImageBoundsIgnoreFraming,
    )?;

    let width = image.width() as u32;
    let height = image.height() as u32;
    if width == 0 || height == 0 {
        return None;
    }
    let bytes_per_row = image.bytes_per_row();
    let data = image.data();
    let bytes: &[u8] = data.bytes();

    let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
    for y in 0..height as usize {
        let row = y * bytes_per_row;
        if row + (width as usize) * 4 > bytes.len() {
            break;
        }
        for x in 0..width as usize {
            let si = row + x * 4;
            let di = (y * width as usize + x) * 4;
            // CGWindowListCreateImageの既定出力はBGRA(premultiplied, ネイティブ
            // バイト順)なので、RGBAへ並べ替える。
            out[di] = bytes[si + 2];
            out[di + 1] = bytes[si + 1];
            out[di + 2] = bytes[si];
            out[di + 3] = bytes[si + 3];
        }
    }
    Some((out, width, height))
}
