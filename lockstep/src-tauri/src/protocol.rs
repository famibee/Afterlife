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
    /// 全機の成功・失敗・取りこぼしの集計をクリアし、連番を数え直す
    Reset,
    /// 生死確認。受けたら ClientMsg::Pong を返す
    Ping,
}

impl ServerMsg {
    /// Click/Key のみ連番を持つ
    pub fn seq(&self) -> Option<u32> {
        match self {
            ServerMsg::Click { seq, .. } | ServerMsg::Key { seq, .. } => Some(*seq),
            ServerMsg::Reset | ServerMsg::Ping => None,
        }
    }
}

/// 子機 → 親機
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    Hello { name: String, os: String },
    /// 直近の操作までの累積結果。reset されるまで fail・missed は減らない
    Status {
        seq: u32,
        ok: u32,
        fail: u32,
        missed: u32,
        first_fail_seq: Option<u32>,
    },
    Pong,
}
