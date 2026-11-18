// インストーラー本体（.dmg/.exe）から、インストールせずに app.asar を取り出す。
// mac(.dmg)はhdiutilだけで完結する。win(.exe、NSIS)は7-Zipが要る
// （計画の実装判断参照：生成ツール利用者の作業機に別途用意してもらう。同梱はしない）。

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum ExtractError {
	Io(io::Error),
	ToolMissing(String),
	NotFound(String),
}

impl std::fmt::Display for ExtractError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Io(e) => write!(f, "入出力エラー: {e}"),
			Self::ToolMissing(m) => write!(f, "{m}"),
			Self::NotFound(m) => write!(f, "{m}"),
		}
	}
}

impl From<io::Error> for ExtractError {
	fn from(e: io::Error) -> Self { Self::Io(e) }
}

/// dmg を読み取り専用マウントし、中の.appから Contents/Resources/app.asar を読んで返す。
/// shelf_core::install.rs の hdiutil attach/detach パターンを踏襲（マウント先の探索・
/// デバイスノードの取得方法が同じ）
pub fn extract_app_asar_from_dmg(dmg_path: &Path) -> Result<Vec<u8>, ExtractError> {
	let output = Command::new("/usr/bin/hdiutil")
		.args(["attach", "-nobrowse", "-readonly"])
		.arg(dmg_path)
		.output()?;
	if !output.status.success() {
		return Err(ExtractError::NotFound(format!(
			"hdiutil attach が失敗した: {}", String::from_utf8_lossy(&output.stderr),
		)));
	}
	let text = String::from_utf8_lossy(&output.stdout);
	let mount_line = text.lines().rev().find(|l| l.contains('\t') && l.contains("/Volumes/"))
		.ok_or_else(|| ExtractError::NotFound("マウントポイントの行が見つからない".to_string()))?;
	let mut fields = mount_line.split('\t').map(str::trim).filter(|s| !s.is_empty());
	let device = fields.next().map(str::to_string);
	let mount_point = fields.last().map(PathBuf::from);

	let result = (|| -> Result<Vec<u8>, ExtractError> {
		let mount_point = mount_point.ok_or_else(|| ExtractError::NotFound("マウントポイントが見つからない".to_string()))?;
		let app_dir = std::fs::read_dir(&mount_point)?
			.filter_map(|e| e.ok())
			.map(|e| e.path())
			.find(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
			.ok_or_else(|| ExtractError::NotFound("dmg内に.appが見つからない".to_string()))?;
		let asar_path = app_dir.join("Contents/Resources/app.asar");
		std::fs::read(&asar_path).map_err(|e| ExtractError::NotFound(format!("{}: {e}", asar_path.display())))
	})();

	if let Some(device) = device {
		let _ = Command::new("/usr/bin/hdiutil").args(["detach", &device]).status();
	}
	result
}

/// PATHを`which`（mac/Linux）・`where`（Windows）で探す。見つからなければ、
/// 7-Zipの既定インストール先（PATHに通っているとは限らない。2026-11-16・Win実機で
/// `C:\Program Files\7-Zip\7z.exe`はあるがPATH未登録と判明）も見る。
fn find_7z_binary() -> Option<PathBuf> {
	#[cfg(windows)]
	let (which_cmd, candidates): (&str, &[&str]) = ("where", &[
		"7z", "7za", "7zz", "7zr",
	]);
	#[cfg(not(windows))]
	let (which_cmd, candidates): (&str, &[&str]) = ("which", &["7z", "7za", "7zz"]);

	for name in candidates {
		if let Ok(output) = Command::new(which_cmd).arg(name).output() {
			if output.status.success() {
				let path = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or("").trim().to_string();
				if !path.is_empty() { return Some(PathBuf::from(path)); }
			}
		}
	}

	#[cfg(windows)]
	let fixed_paths: &[&str] = &[
		r"C:\Program Files\7-Zip\7z.exe",
		r"C:\Program Files (x86)\7-Zip\7z.exe",
	];
	#[cfg(target_os = "macos")]
	let fixed_paths: &[&str] = &["/usr/local/bin/7z", "/opt/homebrew/bin/7z"];
	#[cfg(not(any(windows, target_os = "macos")))]
	let fixed_paths: &[&str] = &[];

	fixed_paths.iter().map(PathBuf::from).find(|p| p.exists())
}

/// `dir`以下を再帰的に探し、ファイル名が`predicate`を満たす最初のパスを返す
fn find_file_recursive(dir: &Path, predicate: impl Fn(&str) -> bool + Copy) -> Option<PathBuf> {
	let entries = std::fs::read_dir(dir).ok()?;
	for entry in entries.filter_map(|e| e.ok()) {
		let path = entry.path();
		if path.is_dir() {
			if let Some(found) = find_file_recursive(&path, predicate) {
				return Some(found);
			}
		} else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
			if predicate(name) {
				return Some(path);
			}
		}
	}
	None
}

fn run_7z_extract(seven_z: &Path, archive: &Path, out_dir: &Path) -> Result<(), ExtractError> {
	let output = Command::new(seven_z)
		.arg("x").arg(archive)
		.arg(format!("-o{}", out_dir.display()))
		.arg("-y")
		.output()?;
	if output.status.success() {
		Ok(())
	} else {
		Err(ExtractError::NotFound(format!(
			"7zの展開に失敗した（{}）: {}", archive.display(), String::from_utf8_lossy(&output.stderr),
		)))
	}
}

/// NSISインストーラー(.exe)はそれ自体が7z形式で展開できるアーカイブ。electron-builderの
/// NSISは`resources/app.asar`を直下に置かず、`$PLUGINSDIR\app-64.7z`
/// （32bit版は`app-32.7z`、ARM64版は`app-arm64.7z`の可能性がある）に二重圧縮して入れている
/// （2026-11-17・Win実機で実配布インストーラー6本すべてこの構造だったと確認）。
/// 一段目を展開→中の`app-*.7z`を探す→二段目を展開、という2段階で取り出す。
pub fn extract_app_asar_from_nsis_exe(exe_path: &Path) -> Result<Vec<u8>, ExtractError> {
	let seven_z = find_7z_binary().ok_or_else(|| ExtractError::ToolMissing(
		"7-Zip（7z/7za/7zz）が見つからない。NSISインストーラーの展開に必要。\
		 mac: `brew install sevenzip`（7zz）、Win: 7-Zip公式インストーラーで入れてください".to_string(),
	))?;

	let out_dir = std::env::temp_dir().join(format!("shelf_gen_extract_{}", std::process::id()));
	std::fs::create_dir_all(&out_dir)?;

	let result = (|| -> Result<Vec<u8>, ExtractError> {
		let stage1 = out_dir.join("stage1");
		run_7z_extract(&seven_z, exe_path, &stage1)?;

		// 直下にresources/app.asarがあれば（電子ビルダーの設定次第ではこの構造もあり得る）
		// それを優先し、無ければ二重圧縮されたapp-*.7zを探して二段目を展開する
		if let Some(direct) = find_file_recursive(&stage1, |n| n == "app.asar") {
			return std::fs::read(&direct).map_err(|e| ExtractError::NotFound(format!("{}: {e}", direct.display())));
		}

		let inner_archive = find_file_recursive(&stage1, |n| {
			n.starts_with("app-") && n.ends_with(".7z")
		}).ok_or_else(|| ExtractError::NotFound(
			"一段目の展開結果にresources/app.asarもapp-*.7zも見つからない".to_string(),
		))?;

		let stage2 = out_dir.join("stage2");
		run_7z_extract(&seven_z, &inner_archive, &stage2)?;

		let asar_path = find_file_recursive(&stage2, |n| n == "app.asar")
			.ok_or_else(|| ExtractError::NotFound(format!("{}の展開結果にapp.asarが見つからない", inner_archive.display())))?;
		std::fs::read(&asar_path).map_err(|e| ExtractError::NotFound(format!("{}: {e}", asar_path.display())))
	})();

	let _ = std::fs::remove_dir_all(&out_dir);
	result
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn extract_app_asar_from_dmg_fails_cleanly_for_missing_file() {
		let result = extract_app_asar_from_dmg(Path::new("/no/such/file.dmg"));
		assert!(result.is_err());
	}

	#[test]
	fn extract_app_asar_from_nsis_exe_reports_missing_tool_or_missing_file() {
		// 7zが入っていない環境では ToolMissing、入っている環境でも存在しないファイルなら
		// 展開失敗になる。どちらにせよ Ok にはならないことだけを確認する
		let result = extract_app_asar_from_nsis_exe(Path::new("/no/such/installer.exe"));
		assert!(result.is_err());
	}

	/// 実配布dmgでの抽出確認（手動）。
	/// `SHELF_TEST_DMG=<dmg> SHELF_TEST_SETTING_SN=<名前> cargo test -p shelf_gen -- --ignored real_dmg`
	#[test]
	#[ignore]
	fn real_dmg_extracts_setting_sn() {
		let dmg = std::env::var("SHELF_TEST_DMG").expect("SHELF_TEST_DMG");
		let name = std::env::var("SHELF_TEST_SETTING_SN").expect("SHELF_TEST_SETTING_SN");
		let asar = extract_app_asar_from_dmg(Path::new(&dmg)).expect("app.asar");
		eprintln!("app.asar: {} bytes", asar.len());
		let mut cursor = std::io::Cursor::new(asar);
		let sn = shelf_core::asar::extract_from_reader(&mut cursor, &name).expect("setting.sn");
		eprintln!("setting.sn: {} bytes", sn.len());
		assert!(!sn.is_empty());
	}

	/// 実物のNSIS exe（要7-Zip）。
	/// `SHELF_TEST_EXE=<exe> SHELF_TEST_SETTING_SN=<名前> cargo test -p shelf_gen -- --ignored --nocapture real_exe`
	#[test]
	#[ignore]
	fn real_exe_extracts_setting_sn() {
		let exe = std::env::var("SHELF_TEST_EXE").expect("SHELF_TEST_EXE");
		let name = std::env::var("SHELF_TEST_SETTING_SN").expect("SHELF_TEST_SETTING_SN");
		let asar = extract_app_asar_from_nsis_exe(Path::new(&exe)).expect("app.asar");
		eprintln!("app.asar: {} bytes", asar.len());
		let mut cursor = std::io::Cursor::new(asar);
		let sn = shelf_core::asar::extract_from_reader(&mut cursor, &name).expect("setting.sn");
		eprintln!("setting.sn: {} bytes, checksum {}", sn.len(), shelf_core::checksum::checksum_hex(&sn));
		assert!(!sn.is_empty());
	}
}
