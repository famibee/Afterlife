use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{ClientMsg, PORT, ServerMsg};

#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    pub id: u64,
    pub name: String,
    pub os: String,
    /// 最後に適用結果が届いた連番
    pub seq: u32,
    pub error: Option<String>,
}

pub struct Host {
    pub tx: broadcast::Sender<ServerMsg>,
    clients: Mutex<HashMap<u64, ClientInfo>>,
}

impl Host {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { tx, clients: Mutex::new(HashMap::new()) }
    }

    fn emit_clients(&self, app: &AppHandle) {
        let mut list: Vec<ClientInfo> = self.clients.lock().unwrap().values().cloned().collect();
        list.sort_by_key(|c| c.id);
        let _ = app.emit("clients", list);
    }
}

pub async fn listen(app: AppHandle, host: Arc<Host>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", PORT)).await?;
    tauri::async_runtime::spawn(async move {
        let mut next_id: u64 = 1;
        while let Ok((stream, _)) = listener.accept().await {
            tauri::async_runtime::spawn(serve(app.clone(), host.clone(), stream, next_id));
            next_id += 1;
        }
    });
    Ok(())
}

async fn serve(app: AppHandle, host: Arc<Host>, stream: TcpStream, id: u64) {
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let (mut sink, mut stream) = ws.split();
    let mut rx = host.tx.subscribe();

    loop {
        tokio::select! {
            out = rx.recv() => match out {
                Ok(msg) => {
                    let Ok(text) = serde_json::to_string(&msg) else { continue };
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                // 取りこぼしは子機側が連番の飛びで検出する
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(msg) = serde_json::from_str::<ClientMsg>(&text) {
                        handle(&app, &host, id, msg);
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }

    host.clients.lock().unwrap().remove(&id);
    host.emit_clients(&app);
}

fn handle(app: &AppHandle, host: &Host, id: u64, msg: ClientMsg) {
    {
        let mut clients = host.clients.lock().unwrap();
        match msg {
            ClientMsg::Hello { name, os } => {
                clients.insert(id, ClientInfo { id, name, os, seq: 0, error: None });
            }
            ClientMsg::Result { seq, error } => {
                if let Some(c) = clients.get_mut(&id) {
                    c.seq = seq;
                    c.error = error;
                }
            }
        }
    }
    host.emit_clients(app);
}
