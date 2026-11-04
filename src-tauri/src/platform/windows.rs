use std::cell::Cell;
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, CreateSolidBrush};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows,
    GetClientRect, GetForegroundWindow, GetMessageW, GetWindow, GetWindowLongW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, KillTimer, PostThreadMessageW,
    RegisterClassExW, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowPos,
    ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, GW_OWNER, GWL_EXSTYLE, HWND_TOPMOST,
    LWA_ALPHA, MSG, SWP_NOACTIVATE, SW_HIDE, SW_RESTORE, SW_SHOWNOACTIVATE, WM_APP, WM_TIMER,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
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

// ---------- クリック位置の視覚フィードバック(オーバーレイ) ----------
//
// Tauriで2枚目のWebView2ウィンドウを同期生成するとデッドロックする既知の問題があるため、
// WebViewを使わず素のWin32レイヤードウィンドウで実装する
// （MicrosoftのPowerToys「Mouse Highlighter」「Find My Mouse」と同じ方式）。
// 専用スレッドで一度だけウィンドウを作り、以降はクリックのたびに
// `PostThreadMessageW`で位置を送って再表示するだけにする。

const CLICK_FEEDBACK_SIZE: i32 = 28;
const CLICK_FEEDBACK_TIMER_ID: usize = 1;
const CLICK_FEEDBACK_TIMER_MS: u32 = 16;
const CLICK_FEEDBACK_ALPHA_STEP: u8 = 17; // 255を15ステップ(約240ms)でフェードアウト
const WM_APP_SHOW_CLICK: u32 = WM_APP + 1;

thread_local! {
    static CLICK_FEEDBACK_ALPHA: Cell<u8> = const { Cell::new(255) };
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn click_feedback_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_TIMER {
            let alpha = CLICK_FEEDBACK_ALPHA.with(|c| {
                let next = c.get().saturating_sub(CLICK_FEEDBACK_ALPHA_STEP);
                c.set(next);
                next
            });
            if alpha == 0 {
                let _ = KillTimer(Some(hwnd), CLICK_FEEDBACK_TIMER_ID);
                let _ = ShowWindow(hwnd, SW_HIDE);
            } else {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
            }
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

/// オーバーレイ専用スレッドを遅延起動し、そのスレッドIDを返す
/// (以後はこのIDへ`PostThreadMessageW`でクリック位置を送るだけにする)。
fn overlay_thread_id() -> u32 {
    static THREAD_ID: OnceLock<u32> = OnceLock::new();
    *THREAD_ID.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<u32>();
        std::thread::spawn(move || {
            let tid = unsafe { GetCurrentThreadId() };
            let _ = tx.send(tid);
            run_overlay_message_loop();
        });
        rx.recv().unwrap_or(0)
    })
}

fn run_overlay_message_loop() {
    unsafe {
        let Ok(hinstance) = GetModuleHandleW(PCWSTR::null()) else {
            return;
        };
        let class_name = to_wide("AfterlifeClickFeedback");
        // BGR順: オレンジがかった赤
        let brush = CreateSolidBrush(COLORREF(0x00_3C5AFF));
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(click_feedback_wnd_proc),
            hInstance: hinstance.into(),
            hbrBackground: brush,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return;
        }

        let Ok(hwnd) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            PCWSTR(class_name.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            CLICK_FEEDBACK_SIZE,
            CLICK_FEEDBACK_SIZE,
            None,
            None,
            Some(hinstance.into()),
            None,
        ) else {
            return;
        };

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

        let mut msg = MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if !ret.as_bool() {
                break;
            }
            if msg.message == WM_APP_SHOW_CLICK {
                // WPARAM/LPARAMはusize/isize(64bit)なので、x/yのビットパターンを
                // u32経由で往復させて符号を保つ(負座標になりうるマルチモニタ環境向け)。
                let x = (msg.wParam.0 as u32) as i32;
                let y = (msg.lParam.0 as u32) as i32;
                let half = CLICK_FEEDBACK_SIZE / 2;
                CLICK_FEEDBACK_ALPHA.with(|c| c.set(255));
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    x - half,
                    y - half,
                    CLICK_FEEDBACK_SIZE,
                    CLICK_FEEDBACK_SIZE,
                    SWP_NOACTIVATE,
                );
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetTimer(Some(hwnd), CLICK_FEEDBACK_TIMER_ID, CLICK_FEEDBACK_TIMER_MS, None);
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// クリック着弾位置(絶対スクリーン座標)に、一瞬表示してフェードアウトする
/// オーバーレイを出す。
pub fn show_click_feedback(x: i32, y: i32) {
    let tid = overlay_thread_id();
    if tid == 0 {
        return;
    }
    unsafe {
        let _ = PostThreadMessageW(
            tid,
            WM_APP_SHOW_CLICK,
            WPARAM((x as u32) as usize),
            LPARAM((y as u32) as isize),
        );
    }
}
