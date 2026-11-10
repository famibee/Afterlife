use std::sync::mpsc::Sender;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{ClientMsg, PORT, ServerMsg};

const RETRY_MAX: Duration = Duration::from_secs(10);

/// 親機へ接続し続ける。切れたら間隔を伸ばしながら繋ぎ直す（止めるにはタスクごと abort）。
pub async fn run(
    app: AppHandle,
    host: String,
    name: String,
    jobs: Sender<ServerMsg>,
    mut out: UnboundedReceiver<ClientMsg>,
) {
    // "192.168.1.5" でも "192.168.1.5:47831" でも受け付ける
    let url = if host.contains(':') {
        format!("ws://{host}/")
    } else {
        format!("ws://{host}:{PORT}/")
    };
    let mut retry = Duration::from_secs(1);

    loop {
        let _ = app.emit("conn", json!({ "state": "connecting", "host": host }));
        if let Ok((ws, _)) = tokio_tungstenite::connect_async(&url).await {
            retry = Duration::from_secs(1);
            let (mut sink, mut stream) = ws.split();
            let hello = ClientMsg::Hello { name: name.clone(), os: std::env::consts::OS.into() };
            if send(&mut sink, &hello).await {
                let _ = app.emit("conn", json!({ "state": "connected", "host": host }));
                loop {
                    tokio::select! {
                        incoming = stream.next() => match incoming {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(msg) = serde_json::from_str::<ServerMsg>(&text) {
                                    let _ = jobs.send(msg);
                                }
                            }
                            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                            Some(Ok(_)) => {}
                        },
                        result = out.recv() => match result {
                            Some(msg) => {
                                if !send(&mut sink, &msg).await {
                                    break;
                                }
                            }
                            None => return,
                        },
                    }
                }
            }
        }
        let _ = app.emit(
            "conn",
            json!({ "state": "retrying", "host": host, "seconds": retry.as_secs() }),
        );
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(RETRY_MAX);
    }
}

async fn send<S>(sink: &mut S, msg: &ClientMsg) -> bool
where
    S: SinkExt<Message> + Unpin,
{
    match serde_json::to_string(msg) {
        Ok(text) => sink.send(Message::Text(text.into())).await.is_ok(),
        Err(_) => true,
    }
}
