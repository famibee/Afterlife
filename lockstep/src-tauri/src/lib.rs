mod inject;
mod local;
mod net;
mod platform;
mod protocol;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use net::server::Host;
use platform::WindowInfo;
use protocol::{ClientMsg, MouseButton, PORT, ServerMsg};

/// 親機で前面をゲームへ移した後、Lockstepへ戻すまでの猶予（注入したキーをゲームが受け取る時間）
const REFOCUS_DELAY: Duration = Duration::from_millis(80);

struct AppState {
    host: Arc<Host>,
    is_host: AtomicBool,
    seq: AtomicU32,
    target: Mutex<Option<WindowInfo>>,
    /// 注入スレッドへの操作キュー。親機のローカル注入も子機の受信も同じキューに積む
    jobs: Mutex<Sender<ServerMsg>>,
    client_out: Mutex<Option<UnboundedSender<ClientMsg>>>,
    client_task: Mutex<Option<JoinHandle<()>>>,
}

/// `--name=value` 引数、なければ `LOCKSTEP_NAME` 環境変数。UI操作なしで起動するためのもの。
fn launch_option(name: &str) -> Option<String> {
    let prefix = format!("--{name}=");
    std::env::args()
        .find_map(|a| a.strip_prefix(&prefix).map(str::to_string))
        .or_else(|| std::env::var(format!("LOCKSTEP_{}", name.to_uppercase())).ok())
        .filter(|v| !v.is_empty())
}

#[tauri::command]
fn initial_config() -> serde_json::Value {
    json!({
        "role": launch_option("role"),
        "host": launch_option("host"),
        "target": launch_option("target"),
        // 自動検証用: 親機が起動後に順に送る操作（例 "ArrowDown,click"）。デバッグビルドのみ
        "autosend": if cfg!(debug_assertions) { launch_option("autosend") } else { None },
    })
}

#[tauri::command]
fn check_permission() -> bool {
    platform::check_input_permission()
}

#[tauri::command]
fn open_permission_settings() {
    platform::open_permission_settings();
}

#[tauri::command]
fn list_windows() -> Vec<WindowInfo> {
    platform::list_windows()
}

#[tauri::command]
fn set_target(state: tauri::State<'_, AppState>, window: Option<WindowInfo>) {
    *state.target.lock().unwrap() = window;
}

fn local_ipv4() -> Option<String> {
    // 実際には通信しない。OSに送信元アドレスを選ばせるだけ
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(addr) => Some(addr.to_string()),
        std::net::IpAddr::V6(_) => None,
    }
}

/// 親機として待ち受けを始め、子機に入力してもらうアドレスを返す。
#[tauri::command]
async fn start_host(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<String, String> {
    if !state.is_host.swap(true, Ordering::SeqCst) {
        if let Err(e) = net::server::listen(app, state.host.clone()).await {
            state.is_host.store(false, Ordering::SeqCst);
            return Err(format!("ポート {PORT} で待ち受けできません: {e}"));
        }
    }
    Ok(local_ipv4().unwrap_or_else(|| "IPアドレス不明".into()))
}

fn host_send(state: &AppState, make: impl FnOnce(u32) -> ServerMsg) -> u32 {
    let seq = state.seq.fetch_add(1, Ordering::SeqCst) + 1;
    let msg = make(seq);
    let _ = state.host.tx.send(msg.clone());
    let _ = state.jobs.lock().unwrap().send(msg);
    seq
}

#[tauri::command]
fn host_click(state: tauri::State<'_, AppState>, x: f32, y: f32, right: bool) -> u32 {
    let button = if right { MouseButton::Right } else { MouseButton::Left };
    host_send(&state, |seq| ServerMsg::Click { seq, x, y, button })
}

#[tauri::command]
fn host_key(state: tauri::State<'_, AppState>, code: String) -> u32 {
    host_send(&state, |seq| ServerMsg::Key { seq, code })
}

#[tauri::command]
fn client_connect(app: AppHandle, state: tauri::State<'_, AppState>, host: String) {
    let mut task = state.client_task.lock().unwrap();
    if let Some(old) = task.take() {
        old.abort();
    }
    let (tx, rx) = unbounded_channel();
    *state.client_out.lock().unwrap() = Some(tx);
    let name = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown".into());
    let jobs = state.jobs.lock().unwrap().clone();
    *task = Some(tauri::async_runtime::spawn(net::client::run(app, host.trim().into(), name, jobs, rx)));
}

/// 操作を1件ずつ順に適用する専用スレッド。前面化の待ちでUIや通信を止めないために分けている。
fn run_injector(app: AppHandle, jobs: Receiver<ServerMsg>) {
    let mut injector = None;
    for msg in jobs {
        let state = app.state::<AppState>();
        let is_host = state.is_host.load(Ordering::SeqCst);
        let target = state.target.lock().unwrap().clone();
        // 親機は対象を選ばなければ操作パッド専用として動く
        if is_host && target.is_none() {
            continue;
        }
        let result = local::apply(&mut injector, target.as_ref(), &msg);
        if is_host && result == Ok(true) {
            std::thread::sleep(REFOCUS_DELAY);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }
        let error = result.err();
        let _ = app.emit("applied", json!({ "seq": msg.seq(), "error": error }));
        if !is_host && let Some(out) = state.client_out.lock().unwrap().as_ref() {
            let _ = out.send(ClientMsg::Result { seq: msg.seq(), error });
        }
    }
}

pub fn run() {
    let (jobs_tx, jobs_rx) = channel();
    tauri::Builder::default()
        .manage(AppState {
            host: Arc::new(Host::new()),
            is_host: AtomicBool::new(false),
            seq: AtomicU32::new(0),
            target: Mutex::new(None),
            jobs: Mutex::new(jobs_tx),
            client_out: Mutex::new(None),
            client_task: Mutex::new(None),
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || run_injector(handle, jobs_rx));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            initial_config,
            check_permission,
            open_permission_settings,
            list_windows,
            set_target,
            start_host,
            host_click,
            host_key,
            client_connect,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
