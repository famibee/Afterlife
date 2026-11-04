use std::collections::HashMap;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use mdns_sd::ServiceDaemon;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, Mutex};
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{ClientMsg, ServerMsg, PORT};

#[derive(Debug, Clone, Serialize)]
pub struct ConnectedClient {
    pub id: u64,
    pub name: String,
    pub os: String,
    pub target: Option<String>,
}

pub struct HostState {
    pub clients: Mutex<HashMap<u64, ConnectedClient>>,
    pub broadcast_tx: broadcast::Sender<ServerMsg>,
    /// mDNS広告の daemon。これを drop すると広告が止まるので保持し続ける。
    pub mdns: Mutex<Option<ServiceDaemon>>,
    pub sidecar: Mutex<Option<SidecarHandle>>,
    pub direct: Mutex<Option<DirectHandle>>,
}

pub struct SidecarHandle {
    pub child: Child,
    pub reader_task: tauri::async_runtime::JoinHandle<()>,
    pub writer_task: tauri::async_runtime::JoinHandle<()>,
}

pub struct DirectHandle {
    pub accept_task: tauri::async_runtime::JoinHandle<()>,
}

impl HostState {
    pub fn new() -> Self {
        let (broadcast_tx, _rx) = broadcast::channel(256);
        Self {
            clients: Mutex::new(HashMap::new()),
            broadcast_tx,
            mdns: Mutex::new(None),
            sidecar: Mutex::new(None),
            direct: Mutex::new(None),
        }
    }

    pub fn send(&self, msg: ServerMsg) {
        // 受信側がいなくてもエラーにしない（誰も繋いでいないだけ）
        let _ = self.broadcast_tx.send(msg);
    }
}

/// `sidecar/host-server.ts` のパスを解決する。開発時はプロジェクトのソースツリー内、
/// パッケージ後はリソースディレクトリ内を見る。
fn resolve_sidecar_script(app: &AppHandle) -> std::io::Result<std::path::PathBuf> {
    // 開発時: Cargo.tomlからの相対パス（ビルド時に埋め込まれる）
    let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sidecar/host-server.ts");
    if dev_path.exists() {
        return Ok(dev_path);
    }
    // パッケージ後: アプリのリソースディレクトリ
    if let Ok(resource_dir) = app.path().resource_dir() {
        let p = resource_dir.join("sidecar/host-server.ts");
        if p.exists() {
            return Ok(p);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "sidecar/host-server.ts not found",
    ))
}

/// Bunサイドカーを起動し、標準入出力経由でWebSocketイベントをやり取りする。
/// 未署名のRustネイティブバイナリが外部からの着信TCPをAvast等に遮断される問題への対処。
pub async fn start_sidecar(app: AppHandle, state: Arc<HostState>) -> std::io::Result<()> {
    let script = resolve_sidecar_script(&app)?;
    eprintln!("[host] spawning bun sidecar: {}", script.display());

    let mut child = Command::new("bun")
        .arg("run")
        .arg(&script)
        .env("AFTERLIFE_PORT", PORT.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;

    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");

    // Rust -> Bun: broadcastキューの内容をstdinへ書き出す
    let mut rx = state.broadcast_tx.subscribe();
    let writer_task = tauri::async_runtime::spawn(async move {
        let mut stdin = stdin;
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let Ok(data) = serde_json::to_string(&msg) else {
                        continue;
                    };
                    let cmd = serde_json::json!({ "cmd": "broadcast", "data": data });
                    let Ok(mut line) = serde_json::to_string(&cmd) else {
                        continue;
                    };
                    line.push('\n');
                    if stdin.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Bun -> Rust: stdoutのイベントを処理する
    let app_for_reader = app.clone();
    let state_for_reader = state.clone();
    let reader_task = tauri::async_runtime::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            handle_sidecar_event(&app_for_reader, &state_for_reader, &line).await;
        }
        eprintln!("[host] sidecar stdout closed");
    });

    *state.sidecar.lock().await = Some(SidecarHandle {
        child,
        reader_task,
        writer_task,
    });

    Ok(())
}

async fn handle_sidecar_event(app: &AppHandle, state: &Arc<HostState>, line: &str) {
    let Ok(event): Result<Value, _> = serde_json::from_str(line) else {
        eprintln!("[host] non-JSON sidecar output: {line}");
        return;
    };
    let Some(kind) = event.get("event").and_then(|v| v.as_str()) else {
        return;
    };
    match kind {
        "listening" => {
            eprintln!("[host] sidecar listening: {line}");
        }
        "open" => {
            // Helloを受け取るまでは正式登録しない。接続自体のログだけ残す。
            eprintln!("[host] sidecar connection opened: {line}");
        }
        "message" => {
            let Some(id) = event.get("id").and_then(|v| v.as_u64()) else {
                return;
            };
            let Some(data) = event.get("data").and_then(|v| v.as_str()) else {
                return;
            };
            handle_client_text(app, state, id, data).await;
        }
        "close" => {
            let Some(id) = event.get("id").and_then(|v| v.as_u64()) else {
                return;
            };
            eprintln!("[host] client#{id}: disconnected");
            state.clients.lock().await.remove(&id);
            emit_clients(app, state).await;
        }
        "error" => {
            eprintln!("[host] sidecar error: {line}");
        }
        _ => {}
    }
}

async fn handle_client_text(app: &AppHandle, state: &Arc<HostState>, id: u64, data: &str) {
    match serde_json::from_str::<ClientMsg>(data) {
        Ok(ClientMsg::Hello { name, os, .. }) => {
            eprintln!("[host] client#{id}: Hello accepted (name={name}, os={os})");
            state.clients.lock().await.insert(
                id,
                ConnectedClient {
                    id,
                    name,
                    os,
                    target: None,
                },
            );
            emit_clients(app, state).await;
        }
        Ok(ClientMsg::Status { target }) => {
            if let Some(c) = state.clients.lock().await.get_mut(&id) {
                c.target = target;
            }
            emit_clients(app, state).await;
        }
        Ok(ClientMsg::Pong) => {}
        Err(e) => {
            eprintln!("[host] client#{id}: failed to parse message: {e} (raw: {data:?})");
        }
    }
}

/// `sidecar`を介さず、Rust本体で直接TCP+WebSocketをlistenする版。
/// Avastの設定変更後、本当にBunサイドカーが必要か切り分けるための検証用。
pub async fn start_direct(app: AppHandle, state: Arc<HostState>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", PORT)).await?;
    eprintln!("[host] direct: listening on 0.0.0.0:{PORT}");

    let state_for_task = state.clone();
    let accept_task = tauri::async_runtime::spawn(async move {
        let mut next_id: u64 = 1;
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[host] direct: accept error: {e}");
                    break;
                }
            };
            let id = next_id;
            next_id += 1;
            eprintln!("[host] direct: accepted connection #{id} from {addr}");
            tauri::async_runtime::spawn(handle_direct_connection(
                app.clone(),
                state_for_task.clone(),
                stream,
                id,
            ));
        }
    });

    *state.direct.lock().await = Some(DirectHandle { accept_task });
    Ok(())
}

async fn handle_direct_connection(
    app: AppHandle,
    state: Arc<HostState>,
    stream: tokio::net::TcpStream,
    id: u64,
) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("[host] direct: WebSocket handshake failed for #{id}: {e}");
            return;
        }
    };
    let (mut sink, mut stream) = ws.split();

    let mut rx = state.broadcast_tx.subscribe();
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();
    let writer_task = tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(msg) => {
                            let Ok(payload) = serde_json::to_string(&msg) else { continue };
                            if sink.send(Message::Text(payload.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = &mut close_rx => break,
            }
        }
        let _ = sink.close().await;
    });

    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                handle_client_text(&app, &state, id, &text).await;
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("[host] direct: connection #{id} read error: {e}");
                break;
            }
        }
    }

    let _ = close_tx.send(());
    let _ = writer_task.await;
    eprintln!("[host] direct: connection #{id} closed");
    state.clients.lock().await.remove(&id);
    emit_clients(&app, &state).await;
}

pub async fn stop_direct(state: &Arc<HostState>) {
    if let Some(handle) = state.direct.lock().await.take() {
        handle.accept_task.abort();
    }
    state.clients.lock().await.clear();
}

async fn emit_clients(app: &AppHandle, state: &Arc<HostState>) {
    let list: Vec<ConnectedClient> = state.clients.lock().await.values().cloned().collect();
    let _ = app.emit("host-clients-changed", list);
}

pub async fn stop_sidecar(state: &Arc<HostState>) {
    if let Some(mut handle) = state.sidecar.lock().await.take() {
        handle.reader_task.abort();
        handle.writer_task.abort();
        let _ = handle.child.kill().await;
    }
    state.clients.lock().await.clear();
}
