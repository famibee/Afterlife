mod input;
mod net;
mod platform;
mod power;
mod protocol;

use std::sync::{Arc, Mutex as StdMutex};

use mdns_sd::ServiceDaemon;
use tauri::{AppHandle, Listener, Manager, WebviewUrl, WebviewWindowBuilder};

use net::client::{self, ClientState, ConnectionStatus};
use net::discovery;
use net::server::{self, HostState};
use protocol::{KeyDirection, MouseButton, ServerMsg, PORT};

struct AppState {
    host: Arc<HostState>,
    client: Arc<ClientState>,
    injector: StdMutex<Option<input::inject::Injector>>,
    power: power::PowerGuard,
    /// 子機: 選んだターゲットウィンドウ
    target: StdMutex<Option<(u64, i32, String)>>, // id, pid, app_name
    client_discovery_daemon: StdMutex<Option<ServiceDaemon>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            host: Arc::new(HostState::new()),
            client: Arc::new(ClientState::new()),
            injector: StdMutex::new(None),
            power: power::PowerGuard::new(),
            target: StdMutex::new(None),
            client_discovery_daemon: StdMutex::new(None),
        }
    }
}

// ---------- 親機 ----------

#[tauri::command]
async fn start_host(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let host = state.host.clone();

    if host.sidecar.lock().await.is_some() {
        return Ok(()); // 既に起動済み
    }
    server::start_sidecar(app.clone(), host.clone())
        .await
        .map_err(|e| e.to_string())?;

    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let name = whoami_hostname();
    discovery::advertise(&daemon, &name).map_err(|e| e.to_string())?;
    *host.mdns.lock().await = Some(daemon);

    Ok(())
}

#[tauri::command]
async fn stop_host(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let host = state.host.clone();
    server::stop_sidecar(&host).await;
    if let Some(daemon) = host.mdns.lock().await.take() {
        // dropするだけでは広告のgoodbyeパケットが送られないことがあるため、明示的にshutdownする
        let _ = daemon.shutdown();
    }
    Ok(())
}

#[tauri::command]
fn host_send_click(state: tauri::State<'_, AppState>, x: f32, y: f32, button: String) {
    let button = if button == "right" {
        MouseButton::Right
    } else {
        MouseButton::Left
    };
    state.host.send(ServerMsg::Click { x, y, button });
}

#[tauri::command]
fn host_send_key(state: tauri::State<'_, AppState>, code: String, direction: String) {
    let direction = if direction == "release" {
        KeyDirection::Release
    } else {
        KeyDirection::Press
    };
    state.host.send(ServerMsg::Key { code, direction });
}

fn whoami_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "Afterlife-Host".to_string())
}

// ---------- 子機: 発見・接続 ----------

#[tauri::command]
fn client_start_discovery(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.client_discovery_daemon.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let daemon_for_thread = daemon.clone();
    *guard = Some(daemon);
    std::thread::spawn(move || {
        discovery::browse_blocking(app, daemon_for_thread);
    });
    Ok(())
}

#[tauri::command]
fn client_stop_discovery(state: tauri::State<'_, AppState>) {
    if let Some(daemon) = state.client_discovery_daemon.lock().unwrap().take() {
        let _ = daemon.shutdown();
    }
}

#[tauri::command]
async fn client_connect(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    host: String,
    port: Option<u16>,
) -> Result<(), String> {
    let client = state.client.clone();
    if let Some(task) = client.task.lock().await.take() {
        task.abort();
    }
    let name = whoami_hostname();
    let app_for_task = app.clone();
    let client_for_task = client.clone();
    let port = port.unwrap_or(PORT);
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(e) = client::connect_and_run(app_for_task, client_for_task, host, port, name).await {
            eprintln!("client connection ended: {e}");
        }
    });
    *client.task.lock().await = Some(handle);
    Ok(())
}

#[tauri::command]
async fn client_disconnect(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    client::disconnect(&app, &state.client).await;
    Ok(())
}

// ---------- 子機: ウィンドウ選択・ハイライト ----------

#[tauri::command]
fn client_list_windows() -> Vec<platform::WindowInfo> {
    platform::list_windows()
}

#[tauri::command]
fn client_check_permission() -> bool {
    platform::check_input_permission()
}

#[tauri::command]
fn client_open_permission_settings() {
    platform::open_permission_settings();
}

#[tauri::command]
async fn client_set_target(
    state: tauri::State<'_, AppState>,
    id: u64,
    pid: i32,
    app_name: String,
) -> Result<(), String> {
    *state.target.lock().unwrap() = Some((id, pid, app_name.clone()));
    client::send_status(&state.client, Some(app_name)).await;
    Ok(())
}

/// ハイライト用オーバーレイの表示操作は、メインウィンドウのキーボードフォーカスを
/// 奪ったまま戻さないことがある（`focused(false)`は生成時のヒントに過ぎず、
/// 表示・非表示の都度フォーカスが移る場合がある）。都度メインウィンドウへ
/// フォーカスを返すことで、入力欄が操作不能になる症状を防ぐ。
fn refocus_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_focus();
    }
}

/// ウィンドウの作成・移動・フォーカス操作はOS側でメインスレッド実行が必須。
/// コマンドのスレッドから直接呼ぶと、特にWindowsでメインループとの間に
/// デッドロックが発生することがあるため、`run_on_main_thread`で処理を委譲する
/// （このコマンド自体は委譲した時点で即座に返り、処理完了を待たない）。
#[tauri::command]
fn client_show_highlight(app: AppHandle, x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
    // Windowsでは、メインスレッド上で2つ目のWebView2を同期生成すると、
    // WebView2自身の非同期初期化がそのスレッドのメッセージポンプを要求するため
    // デッドロックする（run_on_main_thread越しでも再現する）。接続テストという
    // 本筋をブロックするため、ネイティブな非WebView実装に置き換えるまで無効化する。
    #[cfg(windows)]
    {
        let _ = (x, y, width, height);
        return Ok(());
    }
    #[cfg(not(windows))]
    app.clone().run_on_main_thread(move || {
        let label = "highlight-overlay";
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.set_position(tauri::LogicalPosition::new(x as f64, y as f64));
            let _ = win.set_size(tauri::LogicalSize::new(width.max(1) as f64, height.max(1) as f64));
            let _ = win.show();
        } else {
            match WebviewWindowBuilder::new(&app, label, WebviewUrl::App("highlight.html".into()))
                .position(x as f64, y as f64)
                .inner_size(width.max(1) as f64, height.max(1) as f64)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .shadow(false)
                .build()
            {
                Ok(win) => {
                    let _ = win.set_ignore_cursor_events(true);
                }
                Err(e) => eprintln!("[client] failed to create highlight overlay: {e}"),
            }
        }
        refocus_main(&app);
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn client_hide_highlight(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    return Ok(());
    #[cfg(not(windows))]
    app.clone()
        .run_on_main_thread(move || {
            if let Some(win) = app.get_webview_window("highlight-overlay") {
                let _ = win.hide();
            }
            refocus_main(&app);
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn client_test_click(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let target = state.target.lock().unwrap().clone();
    let Some((id, _pid, _)) = target else {
        return Err("ウィンドウが選択されていません".to_string());
    };
    perform_click(&state, id, 0.5, 0.5, MouseButton::Left)
}

fn perform_click(
    state: &tauri::State<'_, AppState>,
    id: u64,
    norm_x: f32,
    norm_y: f32,
    button: MouseButton,
) -> Result<(), String> {
    let Some(rect) = platform::window_rect(id) else {
        return Err("ウィンドウが見つかりません".to_string());
    };
    let target_pid = state.target.lock().unwrap().as_ref().map(|t| t.1);
    let saved = platform::frontmost_window();

    if let Some(pid) = target_pid {
        platform::activate_window(id, pid);
    }

    {
        let mut guard = state.injector.lock().unwrap();
        if guard.is_none() {
            *guard = Some(input::inject::Injector::new().map_err(|e| e.to_string())?);
        }
        let injector = guard.as_ref().unwrap();
        injector
            .click_in_rect(rect, norm_x, norm_y, button)
            .map_err(|e| e.to_string())?;
    }

    if let Some(handle) = saved {
        platform::restore_focus(handle);
    }
    Ok(())
}

fn handle_server_msg(app: &AppHandle, state: &tauri::State<'_, AppState>, msg: ServerMsg) {
    let target_id = state.target.lock().unwrap().as_ref().map(|t| t.0);
    let Some(id) = target_id else { return };
    match msg {
        ServerMsg::Click { x, y, button } => {
            if let Err(e) = perform_click(state, id, x, y, button) {
                eprintln!("click failed: {e}");
            }
        }
        ServerMsg::Key { code, direction } => {
            let guard = state.injector.lock().unwrap();
            if let Some(injector) = guard.as_ref() {
                let _ = injector.key(&code, direction);
            }
        }
        ServerMsg::Ping => {}
    }
    let _ = app; // reserved for future UI feedback events
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .setup(|app| {
            let handle = app.handle().clone();
            let listen_handle = handle.clone();
            app.listen("server-msg", move |event| {
                let Ok(msg) = serde_json::from_str::<ServerMsg>(event.payload()) else {
                    return;
                };
                let app = listen_handle.clone();
                let state = app.state::<AppState>();
                handle_server_msg(&app, &state, msg);
            });

            // 子機が稼働中はスリープを防止する
            let power_handle = handle.clone();
            app.listen("client-connection-changed", move |event| {
                let Ok(status) = serde_json::from_str::<ConnectionStatus>(event.payload()) else {
                    return;
                };
                let state = power_handle.state::<AppState>();
                match status {
                    ConnectionStatus::Connected => state.power.enable(),
                    ConnectionStatus::Disconnected | ConnectionStatus::Failed => state.power.disable(),
                    ConnectionStatus::Connecting => {}
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_host,
            stop_host,
            host_send_click,
            host_send_key,
            client_start_discovery,
            client_stop_discovery,
            client_connect,
            client_disconnect,
            client_list_windows,
            client_check_permission,
            client_open_permission_settings,
            client_set_target,
            client_show_highlight,
            client_hide_highlight,
            client_test_click,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
