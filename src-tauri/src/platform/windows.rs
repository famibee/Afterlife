use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClientRect, GetForegroundWindow, GetWindow,
    GetWindowLongW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
    ShowWindow, GW_OWNER, GWL_EXSTYLE, SW_RESTORE, WS_EX_TOOLWINDOW,
};

use super::WindowInfo;

/// `EnumWindows`のコールバック経由で候補ウィンドウを集める。
unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    unsafe {
        let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);
        if let Some(info) = describe_window(hwnd) {
            out.push(info);
        }
    }
    windows::core::BOOL(1) // 列挙を継続する
}

fn describe_window(hwnd: HWND) -> Option<WindowInfo> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        // オーナーを持つウィンドウ（ダイアログ等）とツールウィンドウは候補から除外する
        if GetWindow(hwnd, GW_OWNER).is_ok() {
            return None;
        }
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }

        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client).ok()?;
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width < 100 || height < 60 {
            return None;
        }

        let mut origin = POINT::default();
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return None; // Afterlife自身のウィンドウ（ハイライトオーバーレイ含む）は候補から除外
        }

        Some(WindowInfo {
            id: hwnd.0 as u64,
            pid: pid as i32,
            app_name: process_name(pid),
            x: origin.x,
            y: origin.y,
            width,
            height,
        })
    }
}

/// pidから実行ファイル名を取得する。失敗した場合は空文字。
fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
        else {
            return String::new();
        };
        let mut buf = [0u16; 260];
        let len = GetModuleBaseNameW(handle, None, &mut buf);
        let _ = CloseHandle(handle);
        if len == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

pub fn list_windows() -> Vec<WindowInfo> {
    let mut result: Vec<WindowInfo> = Vec::new();
    unsafe {
        let ptr = &mut result as *mut Vec<WindowInfo> as isize;
        let _ = EnumWindows(Some(enum_proc), LPARAM(ptr));
    }
    result
}

pub fn window_rect(id: u64) -> Option<(i32, i32, i32, i32)> {
    let hwnd = HWND(id as *mut std::ffi::c_void);
    unsafe {
        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client).ok()?;
        let mut origin = POINT::default();
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }
        Some((
            origin.x,
            origin.y,
            client.right - client.left,
            client.bottom - client.top,
        ))
    }
}

pub fn activate_window(id: u64, _pid: i32) -> bool {
    let hwnd = HWND(id as *mut std::ffi::c_void);
    unsafe {
        // `SetForegroundWindow` は呼び出し元が前面スレッドでないと黙って失敗することがあるため、
        // 対象スレッドの入力キューに一時的に接続してから呼ぶ。
        let mut target_pid: u32 = 0;
        let target_thread = GetWindowThreadProcessId(hwnd, Some(&mut target_pid));
        let current_thread = GetCurrentThreadId();
        let attached = target_thread != 0
            && target_thread != current_thread
            && AttachThreadInput(current_thread, target_thread, true).as_bool();

        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = BringWindowToTop(hwnd);
        let ok = SetForegroundWindow(hwnd).as_bool();

        if attached {
            let _ = AttachThreadInput(current_thread, target_thread, false);
        }
        ok
    }
}

/// 現在の前面ウィンドウのHWNDを返す（復帰用）。
pub fn frontmost_window() -> Option<u64> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        None
    } else {
        Some(hwnd.0 as u64)
    }
}

pub fn restore_focus(handle: u64) -> bool {
    activate_window(handle, 0)
}
