// Shelf生成ツール本体。判定ロジック・フォーマットは全てshelf_coreに委ね、ここは
// プロジェクトスキャン・インストーラー抽出・stubビルド・R2アップロードの配線だけを持つ。

mod extract;
mod gen_state;
mod project_scan;
mod r2;
mod signing_key;
mod stub_build;

use serde::{Deserialize, Serialize};
use shelf_core::manifest::{CircleInfo, OsArtifact, StoreInfo};
use std::path::Path;
use std::process::Command;
use tauri_plugin_dialog::DialogExt;

// --- フォルダ/ファイル選択（tauri-plugin-dialog。旧patch_gen_guiと同じchannel経由パターン） ---

#[tauri::command]
async fn select_folder(app: tauri::AppHandle) -> Option<String> {
	let (tx, rx) = std::sync::mpsc::channel();
	app.dialog().file().pick_folder(move |result| { let _ = tx.send(result); });
	rx.recv().ok().flatten().map(|p| p.to_string())
}

#[tauri::command]
async fn select_installer_files(app: tauri::AppHandle, ext: String) -> Vec<String> {
	let (tx, rx) = std::sync::mpsc::channel();
	app.dialog().file().add_filter("インストーラー", &[&ext]).pick_files(move |result| { let _ = tx.send(result); });
	rx.recv().ok().flatten().unwrap_or_default().into_iter().map(|p| p.to_string()).collect()
}

#[tauri::command]
async fn select_single_file(app: tauri::AppHandle) -> Option<String> {
	let (tx, rx) = std::sync::mpsc::channel();
	app.dialog().file().pick_file(move |result| { let _ = tx.send(result); });
	rx.recv().ok().flatten().map(|p| p.to_string())
}

// --- プロジェクトスキャン ---

#[tauri::command]
fn scan_project_folder(path: String) -> project_scan::ProjectScanResult {
	project_scan::scan_project_folder(Path::new(&path))
}

#[tauri::command]
fn scan_build_installers(path: String) -> project_scan::BuildScanResult {
	project_scan::scan_build_installers(Path::new(&path))
}

// --- インストーラーからの購入者チェック用情報の抽出 ---

#[derive(Debug, Serialize)]
struct SettingSnExtractionResult {
	#[serde(rename = "checksumHex")]
	checksum_hex: String,
}

#[tauri::command]
async fn extract_setting_sn_from_dmg(dmg_path: String, setting_sn_file_name: String) -> Result<SettingSnExtractionResult, String> {
	tauri::async_runtime::spawn_blocking(move || {
		let asar_bytes = extract::extract_app_asar_from_dmg(Path::new(&dmg_path)).map_err(|e| e.to_string())?;
		let mut cursor = std::io::Cursor::new(asar_bytes);
		let setting_sn = shelf_core::asar::extract_from_reader(&mut cursor, &setting_sn_file_name).map_err(|e| e.to_string())?;
		Ok(SettingSnExtractionResult { checksum_hex: shelf_core::checksum::checksum_hex(&setting_sn) })
	}).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn extract_setting_sn_from_nsis_exe(exe_path: String, setting_sn_file_name: String) -> Result<SettingSnExtractionResult, String> {
	tauri::async_runtime::spawn_blocking(move || {
		let asar_bytes = extract::extract_app_asar_from_nsis_exe(Path::new(&exe_path)).map_err(|e| e.to_string())?;
		let mut cursor = std::io::Cursor::new(asar_bytes);
		let setting_sn = shelf_core::asar::extract_from_reader(&mut cursor, &setting_sn_file_name).map_err(|e| e.to_string())?;
		Ok(SettingSnExtractionResult { checksum_hex: shelf_core::checksum::checksum_hex(&setting_sn) })
	}).await.map_err(|e| e.to_string())?
}

/// ②フォールバック用：インストーラー本体のサンプリングチェックサム
/// （shelf_core::inspect::verify_legacy_installerが照合に使う形式と同じ）
#[tauri::command]
fn installer_checksum(path: String) -> Result<String, String> {
	shelf_core::checksum::sampled_file_checksum(Path::new(&path)).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileInfo {
	sha256: String,
	size: u64,
}

/// 最新版インストーラーのR2アップロードキー・マニフェストのOsArtifact（sha256・size）に使う、
/// ファイル全体のチェックサム（完全性検証用。installer_checksumのサンプリング版とは別目的）
#[tauri::command]
async fn file_info(path: String) -> Result<FileInfo, String> {
	tauri::async_runtime::spawn_blocking(move || {
		let path = Path::new(&path);
		let sha256 = shelf_core::checksum::file_checksum(path).map_err(|e| e.to_string())?;
		let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
		Ok(FileInfo { sha256, size })
	}).await.map_err(|e| e.to_string())?
}

// --- 署名鍵 ---

#[tauri::command]
fn load_signing_public_key() -> Result<String, String> {
	signing_key::load_or_generate().map(|k| k.public_key_b64).map_err(|e| e.to_string())
}

// --- ランチャー生成 ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateAppInput {
	app_id: String,
	app_name: String,
	setting_sn_file_name: String,
	#[serde(default)]
	checksum_setting: Vec<String>,
	#[serde(default)]
	checksum_installer: Vec<String>,
	name: String,
	#[serde(default)]
	summary: String,
	#[serde(default)]
	icon_png_base64: String,
	version: String,
	#[serde(default)]
	mac: Option<OsArtifact>,
	#[serde(default)]
	win: Option<OsArtifact>,
	#[serde(default)]
	store: Option<StoreInfo>,
	#[serde(default)]
	event_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateRequest {
	launcher_name: String,
	manifest_url: String,
	manual_url: String,
	circle: CircleInfo,
	apps: Vec<GenerateAppInput>,
	output_dir: String,
	/// mac dmgのボリューム名・出力ファイル名の基準（通例サークル名やランチャー名）
	volume_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateResult {
	mac_dmg_path: Option<String>,
	win_exe_path: Option<String>,
	win_skipped_reason: Option<String>,
	manifest_json: String,
	manifest_signature_b64: String,
	manifest_public_key_b64: String,
}

fn generated_at_now() -> String {
	// chrono等の日付クレートを増やさず、OS標準のdateコマンドで済ませる
	// （依存を増やさない方針。macでのみ想定どおり動く。失敗時は空文字列でも実害はない
	// ——この値はマニフェストの情報欄で、検証ロジックには使われない）
	Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"]).output()
		.ok()
		.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
		.unwrap_or_default()
}

#[tauri::command]
async fn generate_launchers(request: GenerateRequest) -> Result<GenerateResult, String> {
	tauri::async_runtime::spawn_blocking(move || generate_launchers_blocking(request))
		.await
		.map_err(|e| e.to_string())?
}

fn generate_launchers_blocking(request: GenerateRequest) -> Result<GenerateResult, String> {
	let key_info = signing_key::load_or_generate().map_err(|e| e.to_string())?;

	let embedded = shelf_core::config::EmbeddedConfig {
		schema: 1,
		launcher_name: request.launcher_name.clone(),
		manifest_url: request.manifest_url.clone(),
		manifest_public_key: key_info.public_key_b64.clone(),
		manual_url: request.manual_url.clone(),
		apps: request.apps.iter().map(|a| shelf_core::config::AppEntry {
			app_id: a.app_id.clone(),
			app_name: a.app_name.clone(),
			setting_sn_file_name: a.setting_sn_file_name.clone(),
			checksum_setting: a.checksum_setting.clone(),
			checksum_installer: a.checksum_installer.clone(),
		}).collect(),
	};
	let config_json = serde_json::to_string(&embedded).map_err(|e| e.to_string())?;

	let manifest = shelf_core::manifest::Manifest {
		schema: 1,
		generated_at: generated_at_now(),
		circle: request.circle.clone(),
		apps: request.apps.iter().map(|a| shelf_core::manifest::ManifestApp {
			app_id: a.app_id.clone(),
			name: a.name.clone(),
			summary: a.summary.clone(),
			icon_png_base64: a.icon_png_base64.clone(),
			latest: shelf_core::manifest::LatestRelease {
				version: a.version.clone(),
				mac: a.mac.clone(),
				win: a.win.clone(),
			},
			store: a.store.clone(),
			event_only: a.event_only,
		}).collect(),
	};
	let manifest_json = serde_json::to_string(&manifest).map_err(|e| e.to_string())?;
	let manifest_signature_b64 = shelf_core::manifest::sign_body(&key_info.signing_key, manifest_json.as_bytes());

	let output_dir = Path::new(&request.output_dir);
	std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

	let mac_dmg_path = {
		let app_template = stub_build::rebuild_mac_universal_app().map_err(|e| e.to_string())?;
		let out_dmg = output_dir.join(format!("{}.dmg", request.volume_name));
		let result = stub_build::stamp_and_resign_mac_app(&app_template, &config_json, &request.volume_name, &out_dmg);
		// app_templateは一時ディレクトリ内（rebuild_mac_universal_appが作ったもの）。
		// stamp_and_resign_mac_appは複製元として参照するだけで消さないので、ここで片付ける
		if let Some(dir) = app_template.parent() { let _ = std::fs::remove_dir_all(dir); }
		result.map_err(|e| e.to_string())?;
		Some(out_dmg.to_string_lossy().to_string())
	};

	let (win_exe_path, win_skipped_reason) = match stub_build::find_win_prebuilt_exe() {
		Some(stub_exe) => {
			let out_exe = output_dir.join(format!("{}.exe", request.volume_name));
			stub_build::stamp_win_exe(&stub_exe, &config_json, &out_exe).map_err(|e| e.to_string())?;
			(Some(out_exe.to_string_lossy().to_string()), None)
		}
		None => (None, Some(
			"Win向けの事前ビルドstubが見つからない（gen/prebuilt/x86_64-pc-windows-msvc/shelf.exe が無い。\
			 Windows実機でビルドしてblues-sync経由で配置してください）".to_string(),
		)),
	};

	Ok(GenerateResult {
		mac_dmg_path,
		win_exe_path,
		win_skipped_reason,
		manifest_json,
		manifest_signature_b64,
		manifest_public_key_b64: key_info.public_key_b64,
	})
}

pub fn run() {
	tauri::Builder::default()
		.plugin(tauri_plugin_dialog::init())
		.invoke_handler(tauri::generate_handler![
			select_folder,
			select_installer_files,
			select_single_file,
			scan_project_folder,
			scan_build_installers,
			extract_setting_sn_from_dmg,
			extract_setting_sn_from_nsis_exe,
			installer_checksum,
			file_info,
			load_signing_public_key,
			generate_launchers,
			r2::r2_load_config,
			r2::r2_save_config,
			r2::r2_public_url,
			r2::r2_object_exists,
			r2::r2_list_objects,
			r2::r2_delete_others_with_prefix,
			r2::r2_upload_file,
			r2::r2_upload_bytes,
			r2::r2_delete_object,
			gen_state::load_gen_state,
			gen_state::save_gen_state,
		])
		.run(tauri::generate_context!())
		.expect("error while running tauri application");
}
