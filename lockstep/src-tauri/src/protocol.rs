use serde::{Deserialize, Serialize};

pub const PORT: u16 = 47831;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
}

/// 親機 → 全機。seq は親機が振る連番で、取りこぼしの検出に使う。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    /// x, y は対象ウィンドウ内の正規化座標 (0.0..=1.0)
    Click { seq: u32, x: f32, y: f32, button: MouseButton },
    /// code は KeyboardEvent.code。押して離す1回のタップとして注入する
    Key { seq: u32, code: String },
}

impl ServerMsg {
    pub fn seq(&self) -> u32 {
        match self {
            ServerMsg::Click { seq, .. } | ServerMsg::Key { seq, .. } => *seq,
        }
    }
}

/// 子機 → 親機
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    Hello { name: String, os: String },
    /// 1操作ごとの適用結果。error が None なら成功
    Result { seq: u32, error: Option<String> },
}
