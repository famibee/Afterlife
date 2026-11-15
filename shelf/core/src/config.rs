// 埋め込み設定の読み込み。生成ツールがビルド時に焼き込む、配布物1本ごとに固有の情報。
// ここに置くのは「配り直さない」ものだけ（購入者チェックの材料＝チェックサム群、
// マニフェストの取得先とその検証に使う公開鍵）。版・ダウンロードURLなど毎リリース
// 変わるものは遠隔マニフェスト側（manifest.rs）に置く。
//
// 持たせ方はOSで非対称（legacy-app-patch.md系の末尾連結方式はWindows限定にした）：
// - mac: `.app/Contents/Resources/shelf-config.json`（末尾連結は署名の封印を壊す。
//   2026-11-16 実機検証：Resources配下への追加だけでも
//   `codesign --verify --deep --strict` が失敗する。生成ツールが config 埋め込み後に
//   `codesign -s - --force --deep` で再署名することが前提）
// - win: 自分自身（exe）の末尾に footer.rs の形式で連結
// - どちらにも無ければ環境変数 `SHELF_CONFIG=<path>`（開発用。生成ツールの出力物を作らずに
//   `tauri dev` で動かすためのもの。リリースビルドでも明示的に付けた場合は使える）

use crate::footer;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
	pub app_id: String,
	pub app_name: String,
	/// asar内でのbasename（フォルダ位置は問わない。legacy-app-patch.md 2026-09-13決定を踏襲）
	pub setting_sn_file_name: String,
	/// 過去出荷分すべて。どれか1つと一致すればよい
	#[serde(default)]
	pub checksum_setting: Vec<String>,
	/// setting.sn抽出に失敗した場合のフォールバック（②：購入者に当時のインストーラー本体を
	/// 選ばせて比較する）。空なら②は使えない
	#[serde(default)]
	pub checksum_installer: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedConfig {
	pub schema: u32,
	pub launcher_name: String,
	pub manifest_url: String,
	/// base64（標準アルファベット）エンコードされた ed25519 公開鍵（32バイト）。
	/// マニフェストの署名検証に使う（manifest.rs参照）
	pub manifest_public_key: String,
	pub manual_url: String,
	pub apps: Vec<AppEntry>,
}

#[derive(Debug)]
pub enum ConfigError {
	NotFound,
	Io(std::io::Error),
	Json(serde_json::Error),
}

impl std::fmt::Display for ConfigError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::NotFound => write!(f, "埋め込み設定が見つからない"),
			Self::Io(e) => write!(f, "設定ファイルの読み込みに失敗: {e}"),
			Self::Json(e) => write!(f, "設定の解析に失敗: {e}"),
		}
	}
}

/// mac: .app の Contents/Resources/shelf-config.json のパスを、実行ファイルのパスから逆算する。
/// `exe_path` は `.../Foo.app/Contents/MacOS/Foo` の形を想定（std::env::current_exe()相当）
fn mac_resource_config_path(exe_path: &Path) -> Option<PathBuf> {
	// Contents/MacOS/<bin> -> Contents/Resources/shelf-config.json
	let contents_dir = exe_path.parent()?.parent()?;	// .../Contents
	Some(contents_dir.join("Resources").join("shelf-config.json"))
}

fn parse(json: &str) -> Result<EmbeddedConfig, ConfigError> {
	serde_json::from_str(json).map_err(ConfigError::Json)
}

/// 設定を読む。優先順位：環境変数 SHELF_CONFIG（開発用）→ OS別の埋め込み先。
/// `exe_path` はテスト容易性のため呼び出し側から渡す（実運用では std::env::current_exe()）。
pub fn load(exe_path: &Path) -> Result<EmbeddedConfig, ConfigError> {
	if let Ok(dev_path) = std::env::var("SHELF_CONFIG") {
		let json = std::fs::read_to_string(&dev_path).map_err(ConfigError::Io)?;
		return parse(&json);
	}

	if cfg!(target_os = "macos") {
		if let Some(path) = mac_resource_config_path(exe_path) {
			if path.exists() {
				let json = std::fs::read_to_string(&path).map_err(ConfigError::Io)?;
				return parse(&json);
			}
		}
	}
	else if cfg!(target_os = "windows") {
		let bytes = std::fs::read(exe_path).map_err(ConfigError::Io)?;
		if let Some(json) = footer::extract_trailing_json(&bytes) {
			return parse(&json);
		}
	}

	Err(ConfigError::NotFound)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn sample_json() -> &'static str {
		r#"{"schema":1,"launcherName":"Test Launcher","manifestUrl":"https://example.com/manifest.json","manifestPublicKey":"base64key","manualUrl":"https://example.com/manual","apps":[{"appId":"a","appName":"App A","settingSnFileName":"x.sn","checksumSetting":["abc"],"checksumInstaller":[]}]}"#
	}

	#[test]
	fn parse_reads_all_fields() {
		let cfg = parse(sample_json()).unwrap();
		assert_eq!(cfg.schema, 1);
		assert_eq!(cfg.launcher_name, "Test Launcher");
		assert_eq!(cfg.apps.len(), 1);
		assert_eq!(cfg.apps[0].app_id, "a");
		assert_eq!(cfg.apps[0].checksum_setting, vec!["abc".to_string()]);
	}

	#[test]
	fn parse_fails_on_broken_json() {
		assert!(matches!(parse("not json"), Err(ConfigError::Json(_))));
	}

	#[test]
	fn mac_resource_config_path_derives_from_macos_binary_path() {
		let exe = Path::new("/Applications/Shelf.app/Contents/MacOS/Shelf");
		let path = mac_resource_config_path(exe).unwrap();
		assert_eq!(path, PathBuf::from("/Applications/Shelf.app/Contents/Resources/shelf-config.json"));
	}

	#[test]
	fn load_prefers_shelf_config_env_var_over_everything_else() {
		let dir = std::env::temp_dir().join(format!("shelf_config_test_{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let config_path = dir.join("dev-config.json");
		std::fs::write(&config_path, sample_json()).unwrap();

		// SAFETY: テスト内でのみ一時的に設定し、テスト終了前に削除する
		unsafe { std::env::set_var("SHELF_CONFIG", &config_path); }
		let result = load(Path::new("/nonexistent/exe"));
		unsafe { std::env::remove_var("SHELF_CONFIG"); }

		let cfg = result.unwrap();
		assert_eq!(cfg.launcher_name, "Test Launcher");
		std::fs::remove_dir_all(&dir).unwrap();
	}
}
