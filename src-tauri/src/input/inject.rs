use enigo::{Coordinate, Direction as EnigoDirection, Enigo, Keyboard, Mouse, Settings};
use std::sync::Mutex;

use crate::protocol::{KeyDirection, MouseButton as ProtoMouseButton};

/// 他アプリ(ゲーム含む)が合成イベントかどうかを判定するためのマーカー値。
/// enigo既定の 100 は他アプリと衝突しうるため独自値を使う。
pub const EVENT_MARKER: usize = 0x4654_4C49; // "AFTL" の略をASCII風に

pub struct Injector {
    enigo: Mutex<Enigo>,
}

impl Injector {
    pub fn new() -> Result<Self, enigo::NewConError> {
        let settings = Settings {
            event_source_user_data: Some(EVENT_MARKER as i64),
            windows_dw_extra_info: Some(EVENT_MARKER),
            ..Settings::default()
        };
        let enigo = Enigo::new(&settings)?;
        Ok(Self {
            enigo: Mutex::new(enigo),
        })
    }

    /// クライアント領域の矩形(絶対座標)と正規化座標から、元のカーソル位置に戻しつつクリックする。
    pub fn click_in_rect(
        &self,
        rect: (i32, i32, i32, i32), // x, y, width, height
        norm_x: f32,
        norm_y: f32,
        button: ProtoMouseButton,
    ) -> enigo::InputResult<()> {
        let (rx, ry, rw, rh) = rect;
        // 正規化座標1.0がクライアント領域の外側(幅/高さそのもの)を指してしまい、
        // 枠の無いウィンドウや画面端では別ウィンドウ・画面外に着弾しうるため、
        // 最終ピクセル(幅-1, 高さ-1)を上限にする。
        let max_x = (rw - 1).max(0) as f32;
        let max_y = (rh - 1).max(0) as f32;
        let abs_x = rx + (norm_x.clamp(0.0, 1.0) * max_x).round() as i32;
        let abs_y = ry + (norm_y.clamp(0.0, 1.0) * max_y).round() as i32;

        let mut enigo = self.enigo.lock().unwrap();
        let original = enigo.location().ok();

        let btn = match button {
            ProtoMouseButton::Left => enigo::Button::Left,
            ProtoMouseButton::Right => enigo::Button::Right,
        };

        enigo.move_mouse(abs_x, abs_y, Coordinate::Abs)?;
        enigo.button(btn, EnigoDirection::Click)?;

        if let Some((ox, oy)) = original {
            let _ = enigo.move_mouse(ox, oy, Coordinate::Abs);
        }
        Ok(())
    }

    pub fn key(&self, code: &str, direction: KeyDirection) -> enigo::InputResult<()> {
        let Some(key) = map_key(code) else {
            return Ok(());
        };
        let dir = match direction {
            KeyDirection::Press => EnigoDirection::Press,
            KeyDirection::Release => EnigoDirection::Release,
        };
        self.enigo.lock().unwrap().key(key, dir)
    }
}

/// KeyboardEvent.code 相当の文字列からenigoのKeyへ変換する。
/// ゲーム操作で使う範囲のみマッピングし、それ以外は None（無視）。
fn map_key(code: &str) -> Option<enigo::Key> {
    use enigo::Key;
    Some(match code {
        "Enter" | "NumpadEnter" => Key::Return,
        "Space" => Key::Space,
        "Escape" => Key::Escape,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "ArrowUp" => Key::UpArrow,
        "ArrowDown" => Key::DownArrow,
        "ArrowLeft" => Key::LeftArrow,
        "ArrowRight" => Key::RightArrow,
        "ControlLeft" | "ControlRight" => Key::Control,
        "ShiftLeft" | "ShiftRight" => Key::Shift,
        "AltLeft" | "AltRight" => Key::Alt,
        _ if code.starts_with("Key") && code.len() == 4 => {
            Key::Unicode(code.chars().nth(3)?.to_ascii_lowercase())
        }
        _ if code.starts_with("Digit") && code.len() == 6 => {
            Key::Unicode(code.chars().nth(5)?)
        }
        _ => return None,
    })
}
