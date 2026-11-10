use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{ClientMsg, PORT, ServerMsg};

/// この間隔で生死確認を送る
const PING_INTERVAL: Duration = Duration::from_secs(5);
/// この間 Pong が無ければ死んでいるとみなして切る（子機が繋ぎ直す）
const PONG_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize)]
pub struct ClientInfo {
    pub id: u64,
    pub name: String,
    pub os: String,
    /// 最後に適用結果が届いた連番
    pub seq: u32,
    pub ok: u32,
    pub fail: u32,
    pub missed: u32,
    pub first_fail_seq: Option<u32>,
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

    /// 全子機の集計表示をクリアする（Reset配布と対にして呼ぶ。実際の値は
    /// 各子機からの次の Status で上書きされるが、古い値を残さないための即時反映）。
    pub fn reset_clients(&self, app: &AppHandle) {
        {
            let mut clients = self.clients.lock().unwrap();
            for c in clients.values_mut() {
                c.seq = 0;
                c.ok = 0;
                c.fail = 0;
                c.missed = 0;
                c.first_fail_seq = None;
            }
        }
        self.emit_clients(app);
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
    let mut ping_timer = tokio::time::interval(PING_INTERVAL);
    ping_timer.tick().await; // 最初のtickは即座に来るので読み捨てる
    let mut last_pong = Instant::now();

    loop {
        tokio::select! {
            _ = ping_timer.tick() => {
                if last_pong.elapsed() > PONG_TIMEOUT {
                    break;
                }
                let Ok(text) = serde_json::to_string(&ServerMsg::Ping) else { continue };
                if sink.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
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
                    match serde_json::from_str::<ClientMsg>(&text) {
                        Ok(ClientMsg::Pong) => last_pong = Instant::now(),
                        Ok(msg) => handle(&app, &host, id, msg),
                        Err(_) => {}
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
                clients.insert(
                    id,
                    ClientInfo { id, name, os, seq: 0, ok: 0, fail: 0, missed: 0, first_fail_seq: None },
                );
            }
            ClientMsg::Status { seq, ok, fail, missed, first_fail_seq } => {
                if let Some(c) = clients.get_mut(&id) {
                    c.seq = seq;
                    c.ok = ok;
                    c.fail = fail;
                    c.missed = missed;
                    c.first_fail_seq = first_fail_seq;
                }
            }
            ClientMsg::Pong => {}
        }
    }
    host.emit_clients(app);
}
