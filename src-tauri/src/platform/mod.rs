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

/// クリック着弾位置(絶対スクリーン座標)に、一瞬表示してフェードアウトする視覚フィードバックを
/// 出す。Windowsのみ実装（素のWin32レイヤードウィンドウ。WebViewは使わない）。
/// macOSはTauriのWebviewWindowで`lib.rs`側が直接処理している。
#[cfg(windows)]
pub fn show_click_feedback(x: i32, y: i32) {
    windows::show_click_feedback(x, y);
}

/// 指定ウィンドウの内容をRGBAで取得する。
#[cfg(target_os = "macos")]
fn capture_window_rgba(id: u64) -> Option<(Vec<u8>, u32, u32)> {
    macos::capture_window_rgba(id)
}
#[cfg(windows)]
fn capture_window_rgba(id: u64) -> Option<(Vec<u8>, u32, u32)> {
    windows::capture_window_rgba(id)
}

/// 単純な最近傍補間で`max_width`以下に縮小する（`max_width`以上ならそのまま）。
fn downscale_rgba(src: &[u8], w: u32, h: u32, max_width: u32) -> (Vec<u8>, u32, u32) {
    if w == 0 || h == 0 || w <= max_width {
        return (src.to_vec(), w, h);
    }
    let scale = max_width as f32 / w as f32;
    let nw = max_width.max(1);
    let nh = (((h as f32) * scale).round() as u32).max(1);
    let mut out = vec![0u8; (nw as usize) * (nh as usize) * 4];
    for y in 0..nh {
        let sy = (((y as f32 + 0.5) / scale) as u32).min(h - 1);
        for x in 0..nw {
            let sx = (((x as f32 + 0.5) / scale) as u32).min(w - 1);
            let si = ((sy * w + sx) * 4) as usize;
            let di = ((y * nw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    (out, nw, nh)
}

/// 指定ウィンドウのスナップショットを撮り、`max_width`以下に縮小してJPEG化する。
/// 親機の操作パッドに定期表示する用途を想定した低画質・低負荷な実装。
pub fn capture_window_jpeg(id: u64, max_width: u32, quality: u8) -> Option<Vec<u8>> {
    let (rgba, w, h) = capture_window_rgba(id)?;
    let (rgba, w, h) = downscale_rgba(&rgba, w, h, max_width);
    let mut buf = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut buf, quality);
    encoder
        .encode(&rgba, w as u16, h as u16, jpeg_encoder::ColorType::Rgba)
        .ok()?;
    Some(buf)
}
