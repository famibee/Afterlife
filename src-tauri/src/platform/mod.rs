use serde::Serialize;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Serialize)]
pub struct WindowInfo {
    /// プラットフォーム固有のウィンドウ識別子（macOS: CGWindowID, Windows: HWNDをusizeにしたもの）
    pub id: u64,
    pub pid: i32,
    pub app_name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 画面上に出ている候補ウィンドウを列挙する。権限なしで動作すること（タイトルは使わない）。
#[cfg(target_os = "macos")]
pub fn list_windows() -> Vec<WindowInfo> {
    macos::list_windows()
}
#[cfg(windows)]
pub fn list_windows() -> Vec<WindowInfo> {
    windows::list_windows()
}

/// ウィンドウの現在のクライアント領域矩形（絶対座標）を取得する。移動・リサイズに追従するため
/// クリックの都度呼ぶ。
#[cfg(target_os = "macos")]
pub fn window_rect(id: u64) -> Option<(i32, i32, i32, i32)> {
    macos::window_rect(id)
}
#[cfg(windows)]
pub fn window_rect(id: u64) -> Option<(i32, i32, i32, i32)> {
    windows::window_rect(id)
}

/// 指定ウィンドウをアクティブ化する。
#[cfg(target_os = "macos")]
pub fn activate_window(id: u64, pid: i32) -> bool {
    macos::activate_window(id, pid)
}
#[cfg(windows)]
pub fn activate_window(id: u64, pid: i32) -> bool {
    windows::activate_window(id, pid)
}

/// 直前の前面ウィンドウを記録する（復帰用）。
#[cfg(target_os = "macos")]
pub fn frontmost_window() -> Option<u64> {
    macos::frontmost_window()
}
#[cfg(windows)]
pub fn frontmost_window() -> Option<u64> {
    windows::frontmost_window()
}

/// `frontmost_window()` で記録したウィンドウへフォーカスを戻す。
#[cfg(target_os = "macos")]
pub fn restore_focus(handle: u64) -> bool {
    macos::restore_focus(handle)
}
#[cfg(windows)]
pub fn restore_focus(handle: u64) -> bool {
    windows::restore_focus(handle)
}

/// 入力合成に必要な権限が付与されているか（macOS: Accessibility、Windowsは常にtrue）。
#[cfg(target_os = "macos")]
pub fn check_input_permission() -> bool {
    macos::check_accessibility_permission()
}
#[cfg(windows)]
pub fn check_input_permission() -> bool {
    true
}

/// 権限設定画面を開く（macOSのみ意味がある）。
#[cfg(target_os = "macos")]
pub fn open_permission_settings() {
    macos::open_accessibility_settings();
}
#[cfg(windows)]
pub fn open_permission_settings() {}
