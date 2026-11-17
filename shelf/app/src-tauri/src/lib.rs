// Shelf（ランチャー）本体。判定ロジックは全て shelf_core 側にあり、ここは
// Tauriコマンドへの薄い配線と、状態管理・イベント通知だけを担う（Lockstepと同じ作り）。

use serde::Serialize;
use shelf_core::{config, detect, inspect, install, launch, manifest, status};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

struct AppState {
	config: config::EmbeddedConfig,
	manifest: Mutex<Option<manifest::Manifest>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CardDto {
	app_id: String,
	app_name: String,
	icon_png_base64: String,
	summary: String,
	status: String,
	installed_version: Option<String>,
	latest_version: Option<String>,
	event_only: bool,
	store_label: Option<String>,
	store_url: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct InitialView {
	launcher_name: String,
	manual_url: String,
	circle_name: Option<String>,
	circle_site_url: Option<String>,
	circle_support_url: Option<String>,
	circle_email: Option<String>,
	cards: Vec<CardDto>,
}

/// 検出先ディレクトリ。開発・検証時にSHELF_MAC_APPS_DIRで実機の/Applicationsを
/// 汚さずに差し替えられる（dev-fixtures/fake-install を指す運用。Lockstepの
/// 「起動時のロール切り替え」と同じ、UI操作なしで検証するための仕組み）
fn mac_apps_dir() -> PathBuf {
	std::env::var("SHELF_MAC_APPS_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/Applications"))
}

fn win_dirs() -> Vec<PathBuf> {
	if let Ok(dir) = std::env::var("SHELF_WIN_DIRS") {
		return dir.split(';').map(PathBuf::from).collect();
	}
	detect::win_install_base_dirs_from_env()
}

fn status_label(status: &status::CardStatus) -> String {
	match status {
		status::CardStatus::Checking => "checking",
		status::CardStatus::UpToDate => "upToDate",
		status::CardStatus::UpdateAvailable => "updateAvailable",
		status::CardStatus::ArchMigration => "archMigration",
		status::CardStatus::NotGenuine => "notGenuine",
		status::CardStatus::NeedsConfirmation => "needsConfirmation",
		status::CardStatus::NotInstalled { event_only: true } => "notInstalledEventOnly",
		status::CardStatus::NotInstalled { event_only: false } => "notInstalled",
		status::CardStatus::ManifestError { locally_installed: true } => "manifestErrorInstalled",
		status::CardStatus::ManifestError { locally_installed: false } => "manifestError",
	}
	.to_string()
}

fn build_card(entry: &config::AppEntry, manifest: Option<&manifest::Manifest>) -> CardDto {
	let platform = detect::host_platform();
	let local = inspect::inspect_local(entry, platform, &mac_apps_dir(), &win_dirs());
	let manifest_app = manifest.and_then(|m| m.app(&entry.app_id));
	let artifact = manifest_app.and_then(|a| a.for_platform(platform));
	let artifact_arch = artifact.map(|a| a.arch.as_str());

	let card_status = status::decide_status(&local, manifest_app, artifact_arch);

	let installed_version = match &local {
		inspect::LocalState::Installed { version, .. } => version.clone(),
		inspect::LocalState::NotGenuine { version, .. } => version.clone(),
		_ => None,
	};

	CardDto {
		app_id: entry.app_id.clone(),
		app_name: entry.app_name.clone(),
		icon_png_base64: manifest_app.map(|a| a.icon_png_base64.clone()).unwrap_or_default(),
		summary: manifest_app.map(|a| a.summary.clone()).unwrap_or_default(),
		status: status_label(&card_status),
		installed_version,
		latest_version: manifest_app.map(|a| a.latest.version.clone()),
		event_only: manifest_app.map(|a| a.event_only).unwrap_or(false),
		store_label: manifest_app.and_then(|a| a.store.as_ref()).map(|s| s.label.clone()),
		store_url: manifest_app.and_then(|a| a.store.as_ref()).map(|s| s.url.clone()),
	}
}

fn manifest_cache_path(app: &AppHandle) -> Option<PathBuf> {
	app.path().app_config_dir().ok().map(|dir| dir.join("manifest-cache.json"))
}

#[tauri::command]
fn initial_view(app: AppHandle, state: tauri::State<'_, AppState>) -> InitialView {
	let cached = manifest_cache_path(&app).and_then(|p| manifest::load_cached(&p));
	if let Some(m) = &cached {
		*state.manifest.lock().unwrap() = Some(m.clone());
	}
	let manifest = state.manifest.lock().unwrap().clone();

	let cards = state.config.apps.iter().map(|entry| build_card(entry, manifest.as_ref())).collect();
	let circle = manifest.as_ref().map(|m| m.circle.clone());

	InitialView {
		launcher_name: state.config.launcher_name.clone(),
		manual_url: state.config.manual_url.clone(),
		circle_name: circle.as_ref().map(|c| c.name.clone()),
		circle_site_url: circle.as_ref().map(|c| c.site_url.clone()),
		circle_support_url: circle.as_ref().map(|c| c.support_url.clone()),
		circle_email: circle.as_ref().map(|c| c.email.clone()),
		cards,
	}
}

/// マニフェストを取り直し、全カードの状態を再計算して `card-changed` イベントで流す。
/// 取得・検証に失敗した場合はキャッシュ（既にあれば）に基づく表示を維持する。
#[tauri::command]
async fn refresh(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
	let manifest_url = state.config.manifest_url.clone();
	let public_key = state.config.manifest_public_key.clone();
	let cache_path = manifest_cache_path(&app);

	let fetch_result = tauri::async_runtime::spawn_blocking(move || {
		manifest::fetch_and_verify(&manifest_url, &public_key)
	})
	.await
	.map_err(|e| e.to_string())?;

	match fetch_result {
		Ok((m, raw)) => {
			if let Some(path) = &cache_path {
				let _ = manifest::save_cache(path, &raw);
			}
			*state.manifest.lock().unwrap() = Some(m);
		}
		Err(e) => {
			// 既存のキャッシュ・状態を維持したまま、エラーだけ記録する
			// （ManifestErrorのカードはbuild_card側が"manifest"のOption次第で出す）
			log::warn!("マニフェストの取得・検証に失敗: {e}");
		}
	}

	let manifest = state.manifest.lock().unwrap().clone();
	for entry in &state.config.apps {
		let card = build_card(entry, manifest.as_ref());
		let _ = app.emit("card-changed", &card);
	}
	Ok(())
}

#[tauri::command]
fn launch_app(app: AppHandle, state: tauri::State<'_, AppState>, app_id: String) -> Result<(), String> {
	let entry = state.config.apps.iter().find(|a| a.app_id == app_id)
		.ok_or_else(|| "不明なアプリIDです".to_string())?;
	let platform = detect::host_platform();
	let install_path = detect::find_installed_app(&entry.app_name, platform, &mac_apps_dir(), &win_dirs())
		.ok_or_else(|| "インストールが見つかりません".to_string())?;

	launch::launch_installed_app(&install_path, platform, &entry.app_name)
		.map_err(|e| e.to_string())?;

	if let Some(window) = app.get_webview_window("main") {
		let _ = window.close();
	}
	Ok(())
}

#[tauri::command]
async fn install_update(app: AppHandle, state: tauri::State<'_, AppState>, app_id: String) -> Result<String, String> {
	let entry = state.config.apps.iter().find(|a| a.app_id == app_id)
		.ok_or_else(|| "不明なアプリIDです".to_string())?
		.clone();
	let manifest = state.manifest.lock().unwrap().clone()
		.ok_or_else(|| "マニフェストが未取得です".to_string())?;
	let manifest_app = manifest.app(&entry.app_id)
		.ok_or_else(|| "マニフェストにこのアプリの情報がありません".to_string())?;
	let platform = detect::host_platform();
	let artifact = manifest_app.for_platform(platform)
		.ok_or_else(|| "このOS向けの配布情報がありません".to_string())?
		.clone();

	let dest_dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("downloads");
	let app_name = entry.app_name.clone();
	let apps_dir = mac_apps_dir();

	let outcome = tauri::async_runtime::spawn_blocking(move || {
		install::install_update(&artifact, &app_name, &dest_dir, &apps_dir)
	})
	.await
	.map_err(|e| e.to_string())?
	.map_err(|e| e.to_string())?;

	// 結果を反映した最新カードを通知する
	let manifest_opt = state.manifest.lock().unwrap().clone();
	let card = build_card(&entry, manifest_opt.as_ref());
	let _ = app.emit("card-changed", &card);

	Ok(match outcome {
		install::InstallOutcome::Installed => "installed".to_string(),
		install::InstallOutcome::DownloadedOnly(_) => "downloadedOnly".to_string(),
	})
}

/// ②フォールバック：自動検出（setting.sn抽出）に失敗したアプリ向けに、購入者自身に
/// 当時のインストーラー本体を選んでもらい、埋め込み済みのチェックサムと照合する。
/// 一致してもカードの状態（LocalState）そのものは変えない——このフォールバックが
/// 確かめるのは「過去に正規購入した証拠が出せるか」だけで、現在インストールされている
/// ものの真正性までは証明しない。利用者には結果だけダイアログで伝える
#[tauri::command]
async fn pick_legacy_installer(app: tauri::AppHandle, state: tauri::State<'_, AppState>, app_id: String) -> Result<String, String> {
	let (app_name, checksums) = {
		let entry = state.config.apps.iter().find(|a| a.app_id == app_id)
			.ok_or_else(|| "不明なアプリIDです".to_string())?;
		(entry.app_name.clone(), entry.checksum_installer.clone())
	};

	// フォアグラウンドのアプリ自身が出す標準ダイアログ（旧osascript方式は、バックグラウンドの
	// 別プロセスが出すダイアログがキーボードフォーカスを取れない問題があった）
	let ext = if cfg!(target_os = "macos") { "dmg" } else { "exe" };
	let (tx, rx) = std::sync::mpsc::channel();
	app.dialog().file()
		.set_title(format!("{app_name} の旧バージョンのインストーラー（.{ext}）を選択してください"))
		.add_filter("インストーラー", &[ext])
		.pick_file(move |result| { let _ = tx.send(result); });
	let Some(file) = rx.recv().ok().flatten() else {
		return Ok("cancelled".to_string());
	};
	let path = file.into_path().map_err(|e| e.to_string())?;

	let matched = inspect::verify_legacy_installer(&path, &checksums)
		.map_err(|e| e.to_string())?;
	Ok(if matched { "confirmed".to_string() } else { "mismatch".to_string() })
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
	#[cfg(target_os = "macos")]
	let result = std::process::Command::new("/usr/bin/open").arg(&url).spawn();
	#[cfg(target_os = "windows")]
	let result = std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn();

	result.map(|_| ()).map_err(|e| e.to_string())
}

pub fn run() {
	tauri::Builder::default()
		.plugin(tauri_plugin_dialog::init())
		.setup(|app| {
			let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
			let config = config::load(&exe_path).map_err(|e| e.to_string())?;
			app.manage(AppState { config, manifest: Mutex::new(None) });
			Ok(())
		})
		.invoke_handler(tauri::generate_handler![
			initial_view,
			refresh,
			launch_app,
			install_update,
			pick_legacy_installer,
			open_url,
		])
		.run(tauri::generate_context!())
		.expect("error while running tauri application");
}
