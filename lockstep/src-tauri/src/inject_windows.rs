use enigo::{Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};

use crate::protocol::MouseButton;

/// 合成イベントの識別子。enigo既定の 100 は他アプリと衝突しうるため独自値にする。
const EVENT_MARKER: usize = 0x4C4B_5354;

pub struct Backend {
    enigo: Enigo,
}

impl Backend {
    pub fn new() -> Result<Self, String> {
        let settings = Settings { windows_dw_extra_info: Some(EVENT_MARKER), ..Settings::default() };
        let enigo = Enigo::new(&settings).map_err(|e| format!("入力注入を初期化できません: {e}"))?;
        Ok(Self { enigo })
    }

    /// カーソルを対象へ動かしてクリックし、元の位置へ戻す。
    pub fn click(&mut self, x: i32, y: i32, button: MouseButton) -> Result<(), String> {
        let original = self.enigo.location().ok();
        let button = match button {
            MouseButton::Left => enigo::Button::Left,
            MouseButton::Right => enigo::Button::Right,
        };
        self.enigo
            .move_mouse(x, y, Coordinate::Abs)
            .and_then(|_| self.enigo.button(button, Direction::Click))
            .map_err(|e| format!("クリックを注入できません: {e}"))?;
        if let Some((ox, oy)) = original {
            let _ = self.enigo.move_mouse(ox, oy, Coordinate::Abs);
        }
        Ok(())
    }

    pub fn tap(&mut self, code: &str) -> Result<(), String> {
        let key = key(code).ok_or_else(|| format!("未対応のキーです: {code}"))?;
        self.enigo
            .key(key, Direction::Click)
            .map_err(|e| format!("キーを注入できません: {e}"))
    }
}

fn key(code: &str) -> Option<Key> {
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
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Home" => Key::Home,
        "End" => Key::End,
        // 文字・数字の仮想キーコードはASCIIの大文字・数字と同じ値
        _ => Key::Other(super::letter_or_digit(code)? as u32),
    })
}
