// 生成ツール自身のUI状態の永続化（登録済みアプリ・サークル情報・プロジェクトフォルダ
// パス等）。旧patch_gen_guiのload_patch_state/save_patch_stateと同じパターン：
// スキーマはJS側が持ち、Rust側は不透明なserde_json::Valueとして素通しするだけ。

use std::path::PathBuf;
use tauri::Manager;

fn gen_state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
	let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
	Ok(dir.join("gen_state.json"))
}

#[tauri::command]
pub fn load_gen_state(app: tauri::AppHandle) -> Option<serde_json::Value> {
	let path = gen_state_path(&app).ok()?;
	let data = std::fs::read_to_string(path).ok()?;
	serde_json::from_str(&data).ok()
}

#[tauri::command]
pub fn save_gen_state(app: tauri::AppHandle, state: serde_json::Value) -> Result<(), String> {
	let path = gen_state_path(&app)?;
	if let Some(dir) = path.parent() {
		std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
	}
	let json = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
	std::fs::write(&path, json).map_err(|e| e.to_string())
}
