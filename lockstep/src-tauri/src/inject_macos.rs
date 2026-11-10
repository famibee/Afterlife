use std::thread::sleep;
use std::time::Duration;

use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton, EventField,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

use crate::protocol::MouseButton;

/// 押してから離すまでの間隔。短すぎると取りこぼすゲームがある
const HOLD: Duration = Duration::from_millis(30);

/// HID層ではなくセッション層へ送る。Deskflow等のキーボード共有ソフトがHID層に張る
/// イベントタップに合成入力を捨てられることを実測したため（enigoはHID層固定）。
const TAP: CGEventTapLocation = CGEventTapLocation::Session;

pub struct Backend {
    source: CGEventSource,
}

impl Backend {
    pub fn new() -> Result<Self, String> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| "入力注入を初期化できません")?;
        Ok(Self { source })
    }

    /// カーソルは動かさない。イベント自身が座標を持つので、対象には移動→押下→解放が届く。
    pub fn click(&mut self, x: i32, y: i32, button: MouseButton) -> Result<(), String> {
        let point = CGPoint::new(x as f64, y as f64);
        let (down, up, cg_button) = match button {
            MouseButton::Left => {
                (CGEventType::LeftMouseDown, CGEventType::LeftMouseUp, CGMouseButton::Left)
            }
            MouseButton::Right => {
                (CGEventType::RightMouseDown, CGEventType::RightMouseUp, CGMouseButton::Right)
            }
        };
        for (kind, pause) in [(CGEventType::MouseMoved, HOLD), (down, HOLD), (up, Duration::ZERO)] {
            let event = CGEvent::new_mouse_event(self.source.clone(), kind, point, cg_button)
                .map_err(|_| "クリックを注入できません")?;
            if !matches!(kind, CGEventType::MouseMoved) {
                event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, 1);
            }
            event.post(TAP);
            sleep(pause);
        }
        Ok(())
    }

    pub fn tap(&mut self, code: &str) -> Result<(), String> {
        let (keycode, flags) = keycode(code).ok_or_else(|| format!("未対応のキーです: {code}"))?;
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(self.source.clone(), keycode, down)
                .map_err(|_| "キーを注入できません")?;
            event.set_flags(flags);
            event.post(TAP);
            if down {
                sleep(HOLD);
            }
        }
        Ok(())
    }
}

/// 物理キーの仮想キーコードと、実キーが伴うフラグ
fn keycode(code: &str) -> Option<(CGKeyCode, CGEventFlags)> {
    // 実機の矢印・ナビゲーションキーはこの2つのフラグを伴う
    let nav = CGEventFlags::CGEventFlagNumericPad | CGEventFlags::CGEventFlagSecondaryFn;
    let none = CGEventFlags::CGEventFlagNull;
    Some(match code {
        "Enter" => (0x24, none),
        "NumpadEnter" => (0x4C, none),
        "Space" => (0x31, none),
        "Escape" => (0x35, none),
        "Tab" => (0x30, none),
        "Backspace" => (0x33, none),
        "ArrowUp" => (0x7E, nav),
        "ArrowDown" => (0x7D, nav),
        "ArrowLeft" => (0x7B, nav),
        "ArrowRight" => (0x7C, nav),
        "PageUp" => (0x74, nav),
        "PageDown" => (0x79, nav),
        "Home" => (0x73, nav),
        "End" => (0x77, nav),
        _ => (letter_keycode(super::letter_or_digit(code)?)?, none),
    })
}

fn letter_keycode(c: char) -> Option<CGKeyCode> {
    // ANSI配列の仮想キーコード 0x00〜0x2E を順に並べたもの（\0 は文字・数字以外のキー）
    const KEYS: &str = "ASDFHGZXCV\0BQWERYT123465\097\080\0OU\0IP\0LJ\0K\0\0\0\0NM";
    KEYS.chars().position(|k| k == c).map(|i| i as CGKeyCode)
}

#[cfg(test)]
mod tests {
    use super::letter_keycode;

    #[test]
    fn keycodes_match_ansi_layout() {
        let expected = [
            ('A', 0x00), ('S', 0x01), ('V', 0x09), ('B', 0x0B), ('Q', 0x0C), ('T', 0x11),
            ('1', 0x12), ('4', 0x15), ('6', 0x16), ('5', 0x17), ('9', 0x19), ('7', 0x1A),
            ('8', 0x1C), ('0', 0x1D), ('O', 0x1F), ('U', 0x20), ('I', 0x22), ('P', 0x23),
            ('L', 0x25), ('J', 0x26), ('K', 0x28), ('N', 0x2D), ('M', 0x2E),
        ];
        for (c, code) in expected {
            assert_eq!(letter_keycode(c), Some(code), "{c}");
        }
    }
}
