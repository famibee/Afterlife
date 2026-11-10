use crate::protocol::MouseButton;

#[cfg(target_os = "macos")]
#[path = "inject_macos.rs"]
mod imp;
#[cfg(windows)]
#[path = "inject_windows.rs"]
mod imp;

pub struct Injector(imp::Backend);

impl Injector {
    pub fn new() -> Result<Self, String> {
        imp::Backend::new().map(Self)
    }

    /// 矩形(絶対座標)内の正規化座標をクリックする。
    pub fn click_in_rect(
        &mut self,
        rect: (i32, i32, i32, i32),
        norm_x: f32,
        norm_y: f32,
        button: MouseButton,
    ) -> Result<(), String> {
        let (rx, ry, rw, rh) = rect;
        // 1.0 が幅・高さそのものを指すと隣のウィンドウに着弾するため、最終ピクセルを上限にする
        let max_x = (rw - 1).max(0) as f32;
        let max_y = (rh - 1).max(0) as f32;
        let x = rx + (norm_x.clamp(0.0, 1.0) * max_x).round() as i32;
        let y = ry + (norm_y.clamp(0.0, 1.0) * max_y).round() as i32;
        self.0.click(x, y, button)
    }

    /// code は KeyboardEvent.code。押して離す1回のタップとして注入する。
    pub fn tap(&mut self, code: &str) -> Result<(), String> {
        self.0.tap(code)
    }
}

/// "KeyA" → 'A'、"Digit5" → '5'
fn letter_or_digit(code: &str) -> Option<char> {
    code.strip_prefix("Key")
        .or_else(|| code.strip_prefix("Digit"))
        .filter(|rest| rest.len() == 1)?
        .chars()
        .next()
        .filter(char::is_ascii_alphanumeric)
}
