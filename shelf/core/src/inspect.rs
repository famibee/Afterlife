// ローカル検査（ネットワークに触らない部分）。購入者チェック（①自動検出）をランチャーUIの
// ワーカースレッドから呼べる形に切り出したもの。
// ダウンロード・インストーラー選択ダイアログ（②フォールバックの実行そのもの）は
// ここには含めない（UI側がボタン押下に応じて別途呼ぶ）。

use crate::{asar, checksum, config::AppEntry, detect};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalState {
	/// 旧版が見つからない（カードを出さない対象）
	NotInstalled,
	/// 自動検出（setting.sn抽出＋チェックサム一致）に成功した
	Installed { checksum: String, arch: Option<detect::Arch>, version: Option<String> },
	/// インストール自体は検出でき、setting.snも読めたが、既知のチェックサムと一致しない
	/// （体験版・改変等。「製品版を確認できません」。起動自体は妨げない）
	NotGenuine { checksum: String, arch: Option<detect::Arch>, version: Option<String> },
	/// setting.sn自体が読めなかった（asarが無い・想定外の構成等）。インストーラー本体
	/// チェックサムによるフォールバック（②）が使える＝購入者に当時のインストーラーを
	/// 選んでもらえば判定できる
	NeedsConfirmation,
}

/// asarルートの package.json から "version" フィールドだけを読む。
/// 失敗（package.json自体が無い・JSONが壊れている等）はNoneに潰す
/// （版が読めなくても購入者チェック自体は成立するため、ここで失敗にしない）
fn read_installed_version(asar_path: &Path) -> Option<String> {
	let bytes = asar::extract_by_path(asar_path, "package.json").ok()?;
	let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
	value.get("version")?.as_str().map(str::to_string)
}

pub fn inspect_local(
	entry: &AppEntry,
	platform: detect::Platform,
	mac_apps_dir: &Path,
	win_dirs: &[PathBuf],
) -> LocalState {
	let Some(install_path) = detect::find_installed_app(&entry.app_name, platform, mac_apps_dir, win_dirs) else {
		return LocalState::NotInstalled;
	};

	// setting.snが読めたかどうかと、読めた場合のチェックサム一致可否は別に扱う
	// （「読めたが一致しない」＝NotGenuine、「読めなかった」＝NeedsConfirmationの材料、
	// という2つの異なる意味を同じNoneに潰さないため）
	let extraction = (|| -> Option<(bool, String, Option<detect::Arch>, Option<String>)> {
		let asar_path = detect::asar_path_for_install(&install_path, platform)?;
		let setting_sn = asar::extract_by_basename(&asar_path, &entry.setting_sn_file_name).ok()?;
		let is_genuine = checksum::matches_any_known_checksum(&setting_sn, &entry.checksum_setting);
		let hash = checksum::checksum_hex(&setting_sn);
		let arch = detect::detect_installed_arch(&install_path, platform, &entry.app_name);
		let version = read_installed_version(&asar_path);
		Some((is_genuine, hash, arch, version))
	})();

	match extraction {
		Some((true, checksum, arch, version)) => LocalState::Installed { checksum, arch, version },
		Some((false, checksum, arch, version)) => LocalState::NotGenuine { checksum, arch, version },
		None => {
			if entry.checksum_installer.is_empty() {
				// setting.snが読めず、購入者チェックの材料（インストーラー本体の
				// チェックサム）も無い＝これ以上は判定できない。カードを出さない
				LocalState::NotInstalled
			} else {
				LocalState::NeedsConfirmation
			}
		}
	}
}

/// ②フォールバック：購入者に当時のインストーラー本体を選んでもらい、埋め込み済みの
/// インストーラーチェックサム（複数の過去出荷分）のどれかと一致するか確かめる。
/// サンプリング方式（checksum::sampled_file_checksum）を使う＝大きいインストーラーでも軽い。
pub fn verify_legacy_installer(path: &Path, checksum_installer: &[String]) -> std::io::Result<bool> {
	let hex = checksum::sampled_file_checksum(path)?;
	Ok(checksum_installer.iter().any(|known| known.eq_ignore_ascii_case(&hex)))
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	fn app_entry(app_name: &str, checksum_setting: Vec<&str>, checksum_installer: Vec<&str>) -> AppEntry {
		AppEntry {
			app_id: app_name.to_lowercase(),
			app_name: app_name.to_string(),
			setting_sn_file_name: "setting.sn".to_string(),
			checksum_setting: checksum_setting.into_iter().map(String::from).collect(),
			checksum_installer: checksum_installer.into_iter().map(String::from).collect(),
		}
	}

	fn setup_installed_app(root: &Path, app_name: &str, asar_bytes: &[u8]) {
		let resources = root.join(format!("{app_name}.app")).join("Contents/Resources");
		fs::create_dir_all(&resources).unwrap();
		fs::write(resources.join("app.asar"), asar_bytes).unwrap();
	}

	#[test]
	fn not_installed_when_app_dir_is_absent() {
		let dir = std::env::temp_dir().join(format!("shelf_inspect_test_notfound_{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();

		let entry = app_entry("NoSuchGame", vec!["abc"], vec![]);
		let result = inspect_local(&entry, detect::Platform::Mac, &dir, &[]);

		assert_eq!(result, LocalState::NotInstalled);
		fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn installed_when_checksum_matches() {
		let dir = std::env::temp_dir().join(format!("shelf_inspect_test_installed_{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();

		let content = b"&const.experiment = false";
		let header = format!(r#"{{"files":{{"setting.sn":{{"size":{},"offset":"0"}}}}}}"#, content.len());
		let asar_bytes = asar::build_fake_asar(&header, content);
		setup_installed_app(&dir, "MyGame", &asar_bytes);

		let known = checksum::checksum_hex(content);
		let entry = app_entry("MyGame", vec![&known], vec![]);
		let result = inspect_local(&entry, detect::Platform::Mac, &dir, &[]);

		assert_eq!(result, LocalState::Installed { checksum: known, arch: None, version: None });
		fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn installed_reads_version_from_root_package_json() {
		let dir = std::env::temp_dir().join(format!("shelf_inspect_test_version_{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();

		let setting_content = b"&const.experiment = false";
		let pkg_json = br#"{"name":"mygame","version":"1.3.0"}"#;
		let mut bodies = Vec::new();
		bodies.extend_from_slice(setting_content);
		bodies.extend_from_slice(pkg_json);
		let header = format!(
			r#"{{"files":{{"setting.sn":{{"size":{},"offset":"0"}},"package.json":{{"size":{},"offset":"{}"}}}}}}"#,
			setting_content.len(), pkg_json.len(), setting_content.len(),
		);
		let asar_bytes = asar::build_fake_asar(&header, &bodies);
		setup_installed_app(&dir, "MyGame", &asar_bytes);

		let known = checksum::checksum_hex(setting_content);
		let entry = app_entry("MyGame", vec![&known], vec![]);
		let result = inspect_local(&entry, detect::Platform::Mac, &dir, &[]);

		assert_eq!(result, LocalState::Installed { checksum: known, arch: None, version: Some("1.3.0".to_string()) });
		fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn not_genuine_when_setting_sn_is_readable_but_checksum_mismatches() {
		// setting.sn自体は読めている（＝体験版などの実在する中身）ので、
		// checksum_installerの有無に関わらずNotGenuine（読めたが一致しない）になる
		let dir = std::env::temp_dir().join(format!("shelf_inspect_test_notgenuine_{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();

		let content = b"&const.experiment = true";	// 体験版扱いの中身（既知チェックサムと不一致）
		let header = format!(r#"{{"files":{{"setting.sn":{{"size":{},"offset":"0"}}}}}}"#, content.len());
		let asar_bytes = asar::build_fake_asar(&header, content);
		setup_installed_app(&dir, "MyGame", &asar_bytes);

		let known = checksum::checksum_hex(b"&const.experiment = false");
		let mismatched_checksum = checksum::checksum_hex(content);
		let entry = app_entry("MyGame", vec![&known], vec![]);	// checksum_installerが空でも関係ない
		let result = inspect_local(&entry, detect::Platform::Mac, &dir, &[]);

		assert_eq!(result, LocalState::NotGenuine { checksum: mismatched_checksum, arch: None, version: None });
		fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn needs_confirmation_when_setting_sn_itself_cannot_be_read_but_installer_fallback_available() {
		// asar自体はあるが、設定されたbasenameのファイルが存在しない＝抽出失敗
		let dir = std::env::temp_dir().join(format!("shelf_inspect_test_needsconfirm_{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();

		let header = r#"{"files":{"other-file.bin":{"size":0,"offset":"0"}}}"#;
		let asar_bytes = asar::build_fake_asar(header, b"");
		setup_installed_app(&dir, "MyGame", &asar_bytes);

		let entry = app_entry("MyGame", vec!["irrelevant"], vec!["installer-checksum"]);
		let result = inspect_local(&entry, detect::Platform::Mac, &dir, &[]);

		assert_eq!(result, LocalState::NeedsConfirmation);
		fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn not_installed_when_setting_sn_cannot_be_read_and_no_installer_fallback() {
		let dir = std::env::temp_dir().join(format!("shelf_inspect_test_nofallback_{}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();

		let header = r#"{"files":{"other-file.bin":{"size":0,"offset":"0"}}}"#;
		let asar_bytes = asar::build_fake_asar(header, b"");
		setup_installed_app(&dir, "MyGame", &asar_bytes);

		let entry = app_entry("MyGame", vec!["irrelevant"], vec![]);	// checksum_installerが空
		let result = inspect_local(&entry, detect::Platform::Mac, &dir, &[]);

		assert_eq!(result, LocalState::NotInstalled);
		fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn verify_legacy_installer_matches_against_sampled_checksum() {
		let path = std::env::temp_dir().join(format!("shelf_inspect_test_legacy_installer_{}", std::process::id()));
		fs::write(&path, b"old installer bytes").unwrap();

		let known_hex = checksum::sampled_file_checksum(&path).unwrap();
		assert!(verify_legacy_installer(&path, &[known_hex]).unwrap());
		assert!(!verify_legacy_installer(&path, &["0000".to_string()]).unwrap());

		fs::remove_file(&path).unwrap();
	}
}
