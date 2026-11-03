use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{ClientMsg, ServerMsg};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Failed,
}

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

pub struct ClientState {
    pub status: Mutex<ConnectionStatus>,
    pub writer: Mutex<Option<WsSink>>,
    pub task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl ClientState {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(ConnectionStatus::Disconnected),
            writer: Mutex::new(None),
            task: Mutex::new(None),
        }
    }
}

async fn set_status(app: &AppHandle, state: &Arc<ClientState>, status: ConnectionStatus) {
    *state.status.lock().await = status;
    let _ = app.emit("client-connection-changed", status);
}

pub async fn connect_and_run(
    app: AppHandle,
    state: Arc<ClientState>,
    host: String,
    port: u16,
    name: String,
) -> Result<(), String> {
    let url = format!("ws://{host}:{port}/");
    eprintln!("[client] connecting to {url}...");
    set_status(&app, &state, ConnectionStatus::Connecting).await;

    let (ws, _resp) = match tokio_tungstenite::connect_async(&url).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[client] WebSocket connect to {url} failed: {e}");
            set_status(&app, &state, ConnectionStatus::Failed).await;
            return Err(e.to_string());
        }
    };
    eprintln!("[client] WebSocket connected to {url}, sending Hello");

    let (mut sink, mut stream) = ws.split();

    let hello = ClientMsg::Hello {
        name,
        os: std::env::consts::OS.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let payload = serde_json::to_string(&hello).unwrap();
    if let Err(e) = sink.send(Message::Text(payload.into())).await {
        eprintln!("[client] failed to send Hello: {e}");
        set_status(&app, &state, ConnectionStatus::Failed).await;
        return Err(e.to_string());
    }

    *state.writer.lock().await = Some(sink);
    eprintln!("[client] Hello sent, marking Connected");
    set_status(&app, &state, ConnectionStatus::Connected).await;

    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let text = text.to_string();
                match serde_json::from_str::<ServerMsg>(&text) {
                    Ok(msg) => {
                        if matches!(msg, ServerMsg::Ping) {
                            send_status(&state, None).await;
                            continue;
                        }
                        let _ = app.emit("server-msg", msg);
                    }
                    Err(e) => {
                        eprintln!("[client] failed to parse ServerMsg: {e} (raw: {text:?})");
                    }
                }
            }
            Ok(Message::Close(_)) => {
                eprintln!("[client] server closed the connection");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("[client] read error: {e}");
                break;
            }
        }
    }

    *state.writer.lock().await = None;
    set_status(&app, &state, ConnectionStatus::Disconnected).await;
    Ok(())
}

pub async fn send_status(state: &Arc<ClientState>, target: Option<String>) {
    let mut guard = state.writer.lock().await;
    if let Some(writer) = guard.as_mut() {
        let msg = ClientMsg::Status { target };
        if let Ok(payload) = serde_json::to_string(&msg) {
            let _ = writer.send(Message::Text(payload.into())).await;
        }
    }
}

pub async fn disconnect(app: &AppHandle, state: &Arc<ClientState>) {
    if let Some(task) = state.task.lock().await.take() {
        task.abort();
    }
    if let Some(mut writer) = state.writer.lock().await.take() {
        let _ = writer.close().await;
    }
    set_status(app, state, ConnectionStatus::Disconnected).await;
}
