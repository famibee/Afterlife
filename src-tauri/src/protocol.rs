use serde::{Deserialize, Serialize};

pub const PORT: u16 = 47821;
pub const MDNS_SERVICE_TYPE: &str = "_afterlife._tcp.local.";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyDirection {
    Press,
    Release,
}

/// 子機 -> 親機
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    Hello {
        name: String,
        os: String,
        version: String,
    },
    Status {
        target: Option<String>,
    },
    /// 操作対象ウィンドウの縮小スナップショット（JPEG、base64）。定期送信する。
    Snapshot {
        data: String,
    },
    Pong,
}

/// 親機 -> 子機（全子機へ同一内容をブロードキャスト）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    /// x, y はゲーム画面内の正規化座標 (0.0..=1.0)
    Click {
        x: f32,
        y: f32,
        button: MouseButton,
    },
    /// code は KeyboardEvent.code 互換の文字列
    Key {
        code: String,
        direction: KeyDirection,
    },
    /// いずれかの子機が報告した操作対象アプリ名。他の子機（既に接続中・これから
    /// 接続する分とも）へ自動設定のヒントとして配る。子機は自分の対象が未設定、
    /// またはこの名前と異なる場合のみウィンドウ一覧から探して自動選択する。
    TargetHint {
        app_name: String,
    },
    Ping,
}
