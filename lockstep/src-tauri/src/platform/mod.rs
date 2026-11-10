use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod imp;
#[cfg(windows)]
#[path = "windows.rs"]
mod imp;

/// 操作対象の候補ウィンドウ。権限なしで取れる情報だけを持つ（タイトルは使わない）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u64,
    pub pid: i32,
    pub app_name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// list_windows: 候補ウィンドウの一覧
/// window_rect: クリック座標の基準矩形（絶対座標）。移動・リサイズに追従するため都度呼ぶ
/// is_frontmost: 対象がいま前面にあるか
/// focus_window: 前面化を要求する。完了は is_frontmost で確かめる
/// check_input_permission / open_permission_settings: 入力注入の権限（macOSのみ意味を持つ）
pub use imp::{
    check_input_permission, focus_window, is_frontmost, list_windows, open_permission_settings,
    window_rect,
};
