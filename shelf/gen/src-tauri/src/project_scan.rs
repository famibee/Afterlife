// 開発者のゲームプロジェクトフォルダを読んで、登録に必要な情報を自動で拾う。
// 旧 patch_gen_gui（famibee/bluesnovel-vsce コミット41c9c1b）の scan_project_folder・
// scan_build_installers 等の移植。Shelf用に簡略化：`pass`/`relPath` は crypto:true の
// ときだけ使う任意情報に格下げし（計画の実装判断参照）、代わりに `iconPngBase64` を追加した。

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScanResult {
	pub app_name: String,
	pub app_slug: String,
	pub crypto: bool,
	pub icon_png_base64: Option<String>,
	pub warnings: Vec<String>,
}

fn read_package_json(project_folder: &Path) -> Option<serde_json::Value> {
	let bytes = std::fs::read(project_folder.join("package.json")).ok()?;
	serde_json::from_slice(&bytes).ok()
}

pub fn scan_project_folder(project_folder: &Path) -> ProjectScanResult {
	let mut result = ProjectScanResult::default();

	let Some(pkg) = read_package_json(project_folder) else {
		result.warnings.push("package.jsonが見つからない".to_string());
		return result;
	};

	// electron-builderはproductNameでインストール先（/Applications/<productName>.app等）を
	// 決めるため、appNameはこれを最優先にする（build.productNameはelectron-builder設定直書き時の
	// フォールバック）
	result.app_name = pkg.get("productName").and_then(|v| v.as_str())
		.or_else(|| pkg.pointer("/build/productName").and_then(|v| v.as_str()))
		.or_else(|| pkg.get("name").and_then(|v| v.as_str()))
		.unwrap_or_default()
		.to_string();
	if result.app_name.is_empty() {
		result.warnings.push("appNameを特定できなかった（package.jsonのproductName/name参照）".to_string());
	}

	// appSlugはR2キー・出力ファイル名に使うASCII名。package.json.name（npm名の慣習でASCII）を使う
	result.app_slug = pkg.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
	if result.app_slug.is_empty() {
		result.warnings.push("appSlug（package.json.name）が見つからない".to_string());
	}

	result.crypto = project_folder.join("doc_crypto/prj").is_dir()
		|| project_folder.join("doc/crypto_prj").is_dir();

	// electron-builderの既定規約（build/icon.png）。無くても致命的ではないので警告のみ
	let icon_path = project_folder.join("build/icon.png");
	result.icon_png_base64 = std::fs::read(&icon_path).ok().map(|bytes| {
		use base64::{engine::general_purpose::STANDARD, Engine as _};
		STANDARD.encode(bytes)
	});
	if result.icon_png_base64.is_none() {
		result.warnings.push("build/icon.pngが見つからない（アイコンなしで続行できる）".to_string());
	}

	result
}

// --- ビルド成果物（過去出荷インストーラー）のスキャン ---
// `build/include/`（フラット）または `build/package/<任意の開発者ラベル>/`（1階層）を見る。
// ファイル名規約 `${name}-${version}-${arch}.${ext}`（electron-builderの既定artifactName）から
// version/archを読む。ラベル自体（"v1.1.1(2026)4tst"等）は解釈しない。

const KNOWN_ARCHES: [&str; 4] = ["x64", "ia32", "arm64", "universal"];

pub fn is_semver(s: &str) -> bool {
	let parts: Vec<&str> = s.split('.').collect();
	parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

pub fn parse_version_tuple(s: &str) -> Option<(u64, u64, u64)> {
	let mut parts = s.splitn(3, '.');
	let a = parts.next()?.parse().ok()?;
	let b = parts.next()?.parse().ok()?;
	let c = parts.next()?.parse().ok()?;
	Some((a, b, c))
}

/// ファイル名から (version, ext) を取り出す。対応拡張子は exe/dmg のみ
pub fn parse_artifact_filename(filename: &str) -> Option<(String, String)> {
	let (stem, ext) = filename.rsplit_once('.')?;
	if ext != "exe" && ext != "dmg" { return None; }

	let parts: Vec<&str> = stem.split('-').collect();
	if parts.len() < 3 { return None; }

	// 体験版は `_ex` サフィックス付きのarchで出荷される慣例（例: "...-1.0.0-x64_ex.exe"）
	let arch_part = parts.last()?.strip_suffix("_ex").unwrap_or(parts.last()?);
	if !KNOWN_ARCHES.contains(&arch_part) { return None; }

	let version_part = parts[parts.len() - 2];
	if !is_semver(version_part) { return None; }

	Some((version_part.to_string(), ext.to_string()))
}

fn list_files(dir: &Path) -> Vec<PathBuf> {
	std::fs::read_dir(dir)
		.map(|entries| {
			entries
				.filter_map(|e| e.ok())
				.map(|e| e.path())
				.filter(|p| p.is_file())
				.collect()
		})
		.unwrap_or_default()
}

fn collect_candidate_files(project_folder: &Path) -> Vec<PathBuf> {
	let mut files = list_files(&project_folder.join("build/include"));
	if let Ok(entries) = std::fs::read_dir(project_folder.join("build/package")) {
		for entry in entries.filter_map(|e| e.ok()) {
			let path = entry.path();
			if path.is_dir() {
				files.extend(list_files(&path));
			}
		}
	}
	files
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct OsScanResult {
	pub latest: Option<String>,
	pub past: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct BuildScanResult {
	pub win: OsScanResult,
	pub mac: OsScanResult,
}

/// (version_tuple, is_trial, path) の集合から、最新（非体験版の中で最大版）と
/// それ以外（過去版・体験版すべて）に分ける。同じ版なら製品版を体験版より優先する
fn build_os_scan_result(mut files: Vec<((u64, u64, u64), bool, String)>) -> OsScanResult {
	files.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

	let latest_index = files.iter().position(|(_, is_trial, _)| !is_trial);
	let mut result = OsScanResult::default();
	for (i, (_, _, path)) in files.into_iter().enumerate() {
		if Some(i) == latest_index {
			result.latest = Some(path);
		}
		else {
			result.past.push(path);
		}
	}
	result
}

pub fn scan_build_installers(project_folder: &Path) -> BuildScanResult {
	let mut win_files = Vec::new();
	let mut mac_files = Vec::new();

	for path in collect_candidate_files(project_folder) {
		let Some(filename) = path.file_name().and_then(|n| n.to_str()) else { continue };
		let Some((version, ext)) = parse_artifact_filename(filename) else { continue };
		let Some(version_tuple) = parse_version_tuple(&version) else { continue };
		let is_trial = filename.rsplit_once('.').map(|(stem, _)| stem.ends_with("_ex")).unwrap_or(false);
		let path_str = path.to_string_lossy().to_string();

		match ext.as_str() {
			"exe" => win_files.push((version_tuple, is_trial, path_str)),
			"dmg" => mac_files.push((version_tuple, is_trial, path_str)),
			_ => {}
		}
	}

	BuildScanResult {
		win: build_os_scan_result(win_files),
		mac: build_os_scan_result(mac_files),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn is_semver_requires_three_numeric_dot_parts() {
		assert!(is_semver("1.2.3"));
		assert!(!is_semver("1.2"));
		assert!(!is_semver("1.2.3.4"));
		assert!(!is_semver("1.x.3"));
	}

	#[test]
	fn parse_artifact_filename_extracts_version_and_ext() {
		assert_eq!(parse_artifact_filename("MyGame-1.3.0-x64.exe"), Some(("1.3.0".to_string(), "exe".to_string())));
		assert_eq!(parse_artifact_filename("my-game-name-2.0.0-universal.dmg"), Some(("2.0.0".to_string(), "dmg".to_string())));
		assert_eq!(parse_artifact_filename("MyGame-1.0.0-x64_ex.exe"), Some(("1.0.0".to_string(), "exe".to_string())));
	}

	#[test]
	fn parse_artifact_filename_rejects_unknown_shapes() {
		assert_eq!(parse_artifact_filename("builder-debug.yml"), None);
		assert_eq!(parse_artifact_filename("MyGame-1.3.0.exe"), None);	// archが無い
		assert_eq!(parse_artifact_filename("MyGame-1.3.0-sparc.exe"), None);	// 未知arch
		assert_eq!(parse_artifact_filename("MyGame-1.3.0-x64.zip"), None);	// 未対応拡張子
	}

	#[test]
	fn build_os_scan_result_picks_highest_non_trial_as_latest() {
		let files = vec![
			((1, 0, 0), false, "v1.exe".to_string()),
			((1, 2, 0), true, "v1.2-trial.exe".to_string()),
			((1, 1, 0), false, "v1.1.exe".to_string()),
		];
		let result = build_os_scan_result(files);
		assert_eq!(result.latest, Some("v1.1.exe".to_string()));
		assert_eq!(result.past.len(), 2);
	}

	#[test]
	fn build_os_scan_result_prefers_production_over_trial_at_same_version() {
		let files = vec![
			((1, 0, 0), true, "trial.exe".to_string()),
			((1, 0, 0), false, "prod.exe".to_string()),
		];
		let result = build_os_scan_result(files);
		assert_eq!(result.latest, Some("prod.exe".to_string()));
		assert_eq!(result.past, vec!["trial.exe".to_string()]);
	}

	#[test]
	fn scan_project_folder_reports_missing_package_json() {
		let dir = std::env::temp_dir().join(format!("shelf_gen_test_scan_missing_{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();

		let result = scan_project_folder(&dir);
		assert!(result.warnings.iter().any(|w| w.contains("package.json")));

		std::fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn scan_project_folder_prefers_product_name_and_detects_crypto() {
		let dir = std::env::temp_dir().join(format!("shelf_gen_test_scan_ok_{}", std::process::id()));
		std::fs::create_dir_all(dir.join("doc_crypto/prj")).unwrap();
		std::fs::write(dir.join("package.json"), r#"{"name":"my-game","productName":"マイゲーム"}"#).unwrap();

		let result = scan_project_folder(&dir);
		assert_eq!(result.app_name, "マイゲーム");
		assert_eq!(result.app_slug, "my-game");
		assert!(result.crypto);
		assert!(result.icon_png_base64.is_none());

		std::fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn scan_build_installers_finds_files_in_include_and_package_subdirs() {
		let dir = std::env::temp_dir().join(format!("shelf_gen_test_scan_build_{}", std::process::id()));
		std::fs::create_dir_all(dir.join("build/include")).unwrap();
		std::fs::create_dir_all(dir.join("build/package/v1.1.1(2026)4tst")).unwrap();
		std::fs::write(dir.join("build/include/MyGame-1.0.0-x64.exe"), b"").unwrap();
		std::fs::write(dir.join("build/package/v1.1.1(2026)4tst/MyGame-1.1.1-x64.exe"), b"").unwrap();
		std::fs::write(dir.join("build/package/v1.1.1(2026)4tst/MyGame-1.1.1-universal.dmg"), b"").unwrap();
		std::fs::write(dir.join("build/package/v1.1.1(2026)4tst/builder-debug.yml"), b"").unwrap();

		let result = scan_build_installers(&dir);
		assert_eq!(result.win.latest.as_deref().map(|p| p.ends_with("MyGame-1.1.1-x64.exe")), Some(true));
		assert_eq!(result.win.past.len(), 1);
		assert_eq!(result.mac.latest.as_deref().map(|p| p.ends_with("MyGame-1.1.1-universal.dmg")), Some(true));

		std::fs::remove_dir_all(&dir).unwrap();
	}
}
