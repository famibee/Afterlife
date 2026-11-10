use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GWL_EXSTYLE, GW_OWNER, GetClientRect, GetForegroundWindow,
    GetWindow, GetWindowLongW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SW_RESTORE, SetForegroundWindow, ShowWindow, WS_EX_TOOLWINDOW,
};

use super::WindowInfo;

fn hwnd(id: u64) -> HWND {
    HWND(id as *mut c_void)
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    unsafe {
        let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);
        if let Some(info) = describe_window(hwnd) {
            out.push(info);
        }
    }
    windows::core::BOOL(1)
}

fn describe_window(hwnd: HWND) -> Option<WindowInfo> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        // オーナーを持つウィンドウ（ダイアログ等）とツールウィンドウは候補にしない
        if GetWindow(hwnd, GW_OWNER).is_ok() {
            return None;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }
        let (x, y, width, height) = window_rect(hwnd.0 as u64)?;
        if width < 100 || height < 60 {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        Some(WindowInfo {
            id: hwnd.0 as u64,
            pid: pid as i32,
            app_name: process_name(pid),
            x,
            y,
            width,
            height,
        })
    }
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
        else {
            return String::new();
        };
        let mut buf = [0u16; 260];
        let len = GetModuleBaseNameW(handle, None, &mut buf);
        let _ = CloseHandle(handle);
        String::from_utf16_lossy(&buf[..len as usize])
    }
}

pub fn list_windows() -> Vec<WindowInfo> {
    let mut result: Vec<WindowInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut result as *mut Vec<WindowInfo> as isize));
    }
    result
}

/// クライアント領域（タイトルバー・枠を除く）を返す。
pub fn window_rect(id: u64) -> Option<(i32, i32, i32, i32)> {
    let hwnd = hwnd(id);
    unsafe {
        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client).ok()?;
        let mut origin = POINT::default();
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }
        Some((origin.x, origin.y, client.right - client.left, client.bottom - client.top))
    }
}

pub fn is_frontmost(id: u64, _pid: i32) -> bool {
    // 最小化したウィンドウは前面扱いのまま残ることがあるので、最小化中は前面とみなさない
    // （みなすと focus_window が呼ばれず復元されないまま注入してしまう）
    unsafe { GetForegroundWindow() == hwnd(id) && !IsIconic(hwnd(id)).as_bool() }
}

pub fn focus_window(id: u64, _pid: i32) {
    let hwnd = hwnd(id);
    unsafe {
        // 最大化を解除しないよう、復元するのは最小化されているときだけ
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // 戻り値だけでは成功を保証しない（trueでも実際には前面化しない実装がある）ため、
        // 実際に前面になったかを都度確かめる
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return;
        }
        // 背面プロセスからの前面化はWindowsに拒否されるため、いま前面にあるスレッドの
        // 入力キューへ一時的に接続して権利を借りる
        let fg_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let current = GetCurrentThreadId();
        if fg_thread == 0 || fg_thread == current {
            return;
        }
        if AttachThreadInput(current, fg_thread, true).as_bool() {
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(current, fg_thread, false);
        }
    }
}

pub fn check_input_permission() -> bool {
    true
}

pub fn open_permission_settings() {}
