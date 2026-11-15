// 配布用ランチャー本体（Win exe・mac app/dmg）のビルドと、サークル固有の設定の埋め込み。
// 旧patch_gen_gui（famibee/bluesnovel-vsce コミット41c9c1b）の cargo_bin/rustc_bin/
// run_cargo_build/build_mac_universal_stub の構成を踏襲し、対象を patch_app から
// shelf/app に差し替えた。旧ツールはpatch_appがTauri未使用だったためlipo手動結合だったが、
// shelf/appはTauriアプリなので `tauri build --target universal-apple-darwin` の
// 内蔵universal結合機能をそのまま使う（lipo手動結合は不要）。

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum StubBuildError {
	Io(io::Error),
	BuildFailed(String),
}

impl std::fmt::Display for StubBuildError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Io(e) => write!(f, "入出力エラー: {e}"),
			Self::BuildFailed(m) => write!(f, "{m}"),
		}
	}
}

impl From<io::Error> for StubBuildError {
	fn from(e: io::Error) -> Self { Self::Io(e) }
}

// rustupが`~/.cargo/bin/cargo`に入っていても、Homebrew版cargoが$PATH上で先に来ている
// 環境がある。Homebrew版はクロスターゲットを認識しないため、存在すれば明示的に
// rustup管理下のcargoを使う（旧patch_gen_guiと同じ罠・同じ対処）
fn rustc_bin() -> PathBuf {
	if let Some(home) = std::env::var_os("HOME") {
		let candidate = PathBuf::from(home).join(".cargo/bin/rustc");
		if candidate.exists() { return candidate; }
	}
	PathBuf::from("rustc")
}

fn cargo_bin_dir() -> Option<PathBuf> {
	std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo/bin"))
}

/// shelf/ ルートディレクトリ（shelf_gen自身の Cargo.toml の2階層上＝shelf/gen/src-tauri → shelf/）
fn shelf_root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR"))
		.parent().unwrap_or(Path::new("."))	// shelf/gen
		.parent().unwrap_or(Path::new("."))	// shelf/
		.to_path_buf()
}

fn shelf_app_dir() -> PathBuf {
	shelf_root().join("app")
}

/// npmでインストール済みのTauri CLI（JS版）。icon生成・dev起動で既に実績のある経路を
/// そのまま使う（`cargo-tauri`バイナリを直接呼ぶ場合の引数規約の不確実性を避ける）
fn tauri_cli_bin() -> PathBuf {
	shelf_root().join("node_modules/.bin/tauri")
}

/// `shelf/app/src-tauri`はworkspaceメンバーなので、ビルド成果物はworkspace共有の
/// `shelf/target/`に出力される（`shelf/app/src-tauri/target/`ではない。最初の実装で
/// 誤って後者を仮定し、2026-11-16の実機検証で.appが見つからず失敗した）
fn workspace_target_dir() -> PathBuf {
	shelf_root().join("target")
}

/// mac向けuniversal2バイナリの.appを`shelf/app`でビルドする。生成のたびに毎回ビルドし直す
/// （stubのスキーマずれ事故を構造的に防ぐ、という旧ツールの教訓を踏襲）。
///
/// ⚠️ 2026-11-16実機検証で判明：`bundle/macos/`配下には.appが残らず（空）、ビルドされた
/// .appは`bundle/dmg/*.dmg`の中にしか存在しない。そのためここでは一旦dmgをマウントして
/// 中の.appを一時ディレクトリへ複製し、そのパスを返す（呼び出し側はこれを「テンプレート」
/// として扱い、さらに書き換えて別のdmgに包み直す）
pub fn rebuild_mac_universal_app() -> Result<PathBuf, StubBuildError> {
	let app_dir = shelf_app_dir();
	let mut cmd = Command::new(tauri_cli_bin());
	cmd.arg("build").arg("--target").arg("universal-apple-darwin");
	cmd.env("RUSTC", rustc_bin());
	if let Some(cargo_bin_dir) = cargo_bin_dir() {
		let path = std::env::var_os("PATH").unwrap_or_default();
		let mut new_path = cargo_bin_dir.into_os_string();
		new_path.push(":");
		new_path.push(path);
		cmd.env("PATH", new_path);
	}
	cmd.current_dir(&app_dir);

	let output = cmd.output()
		.map_err(|e| StubBuildError::BuildFailed(format!("tauri CLIの起動に失敗（node/npm installが必要かも）: {e}")))?;
	if !output.status.success() {
		return Err(StubBuildError::BuildFailed(String::from_utf8_lossy(&output.stderr).to_string()));
	}

	let dmg_dir = workspace_target_dir().join("universal-apple-darwin/release/bundle/dmg");
	let dmg_path = std::fs::read_dir(&dmg_dir)
		.map_err(|e| StubBuildError::BuildFailed(format!("{}: {e}", dmg_dir.display())))?
		.filter_map(|e| e.ok())
		.map(|e| e.path())
		.find(|p| p.extension().and_then(|e| e.to_str()) == Some("dmg"))
		.ok_or_else(|| StubBuildError::BuildFailed(format!("ビルドは成功したはずだがdmgが見つからない: {}", dmg_dir.display())))?;

	extract_app_from_dmg(&dmg_path)
}

/// ビルド直後のdmgから.appを一時ディレクトリへ取り出す（テンプレートとして複製元に使うため）
fn extract_app_from_dmg(dmg_path: &Path) -> Result<PathBuf, StubBuildError> {
	let output = Command::new("/usr/bin/hdiutil").args(["attach", "-nobrowse", "-readonly"]).arg(dmg_path).output()?;
	if !output.status.success() {
		return Err(StubBuildError::BuildFailed(format!("hdiutil attachが失敗した: {}", String::from_utf8_lossy(&output.stderr))));
	}
	let text = String::from_utf8_lossy(&output.stdout);
	let mount_line = text.lines().rev().find(|l| l.contains('\t') && l.contains("/Volumes/"))
		.ok_or_else(|| StubBuildError::BuildFailed("マウントポイントの行が見つからない".to_string()))?;
	let mut fields = mount_line.split('\t').map(str::trim).filter(|s| !s.is_empty());
	let device = fields.next().map(str::to_string);
	let mount_point = fields.last().map(PathBuf::from);

	let result = (|| -> Result<PathBuf, StubBuildError> {
		let mount_point = mount_point.ok_or_else(|| StubBuildError::BuildFailed("マウントポイントが見つからない".to_string()))?;
		let app_in_dmg = std::fs::read_dir(&mount_point)?
			.filter_map(|e| e.ok())
			.map(|e| e.path())
			.find(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
			.ok_or_else(|| StubBuildError::BuildFailed("dmg内に.appが見つからない".to_string()))?;

		let out_dir = std::env::temp_dir().join(format!("shelf_gen_template_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&out_dir);
		std::fs::create_dir_all(&out_dir)?;
		let out_app = out_dir.join(app_in_dmg.file_name().unwrap());
		let ditto = Command::new("/usr/bin/ditto").arg(&app_in_dmg).arg(&out_app).status()?;
		if !ditto.success() {
			return Err(StubBuildError::BuildFailed("dittoによる.appの複製に失敗した".to_string()));
		}
		Ok(out_app)
	})();

	if let Some(device) = device {
		let _ = Command::new("/usr/bin/hdiutil").args(["detach", &device]).status();
	}
	result
}

/// ビルド済みの universal .app に、サークル固有の設定（EmbeddedConfig のJSON）を
/// Contents/Resources/shelf-config.json として埋め込み、ad-hoc再署名してからdmg化する。
///
/// ⚠️ 2026-11-16 M6実機検証：ad-hoc署名の.appはResources配下へのファイル追加だけで
/// 封印が壊れる（`codesign --verify --deep --strict` が失敗する）。再署名で解消する。
pub fn stamp_and_resign_mac_app(
	app_template: &Path,
	config_json: &str,
	volume_name: &str,
	out_dmg: &Path,
) -> Result<(), StubBuildError> {
	let work_dir = std::env::temp_dir().join(format!("shelf_gen_stamp_{}", std::process::id()));
	std::fs::create_dir_all(&work_dir)?;
	let out_app = work_dir.join(format!("{volume_name}.app"));
	let _ = std::fs::remove_dir_all(&out_app);

	let result = (|| -> Result<(), StubBuildError> {
		let ditto = Command::new("/usr/bin/ditto").arg(app_template).arg(&out_app).status()?;
		if !ditto.success() {
			return Err(StubBuildError::BuildFailed("dittoによる.appの複製に失敗した".to_string()));
		}

		let resources = out_app.join("Contents/Resources");
		std::fs::write(resources.join("shelf-config.json"), config_json)?;

		let codesign = Command::new("/usr/bin/codesign")
			.args(["-s", "-", "--force", "--deep"])
			.arg(&out_app)
			.status()?;
		if !codesign.success() {
			return Err(StubBuildError::BuildFailed("codesignによる再署名に失敗した".to_string()));
		}

		if let Some(parent) = out_dmg.parent() {
			std::fs::create_dir_all(parent)?;
		}
		let _ = std::fs::remove_file(out_dmg);
		let hdiutil = Command::new("/usr/bin/hdiutil")
			.args(["create", "-volname", volume_name, "-srcfolder"])
			.arg(&out_app)
			.args(["-ov", "-format", "UDZO"])
			.arg(out_dmg)
			.status()?;
		if !hdiutil.success() {
			return Err(StubBuildError::BuildFailed("hdiutil createによるdmg作成に失敗した".to_string()));
		}
		Ok(())
	})();

	let _ = std::fs::remove_dir_all(&work_dir);
	result
}

/// Win向けの事前ビルド済みstubを探す（旧ツールと同じ運用：Windows実機でビルドし、
/// blues-sync経由でこのパスに配置する。クロスコンパイルは引き続き不可）
pub fn find_win_prebuilt_exe() -> Option<PathBuf> {
	let candidate = shelf_root().join("gen/prebuilt/x86_64-pc-windows-msvc/shelf.exe");
	if candidate.exists() { Some(candidate) } else { None }
}

/// Winのstub exeの末尾に設定JSONを連結する（shelf_core::footer参照）
pub fn stamp_win_exe(stub_exe: &Path, config_json: &str, out_exe: &Path) -> io::Result<()> {
	let stub_bytes = std::fs::read(stub_exe)?;
	let stamped = shelf_core::footer::append_footer(&stub_bytes, config_json);
	if let Some(parent) = out_exe.parent() {
		std::fs::create_dir_all(parent)?;
	}
	std::fs::write(out_exe, stamped)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn stamp_win_exe_writes_a_file_whose_footer_round_trips() {
		let dir = std::env::temp_dir().join(format!("shelf_gen_test_stub_build_{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let stub_path = dir.join("stub.exe");
		std::fs::write(&stub_path, b"fake-stub-bytes").unwrap();
		let out_path = dir.join("out.exe");

		let config_json = r#"{"schema":1,"apps":[]}"#;
		stamp_win_exe(&stub_path, config_json, &out_path).unwrap();

		let out_bytes = std::fs::read(&out_path).unwrap();
		assert_eq!(shelf_core::footer::extract_trailing_json(&out_bytes).as_deref(), Some(config_json));

		std::fs::remove_dir_all(&dir).unwrap();
	}

	// 重い実機検証（mac universalビルド〜再署名〜dmg化の通し）。`cargo test -- --ignored`で
	// 明示的に実行する（数分かかる＋`shelf/app`のビルドに依存するため、通常のcargo testには含めない）。
	#[test]
	#[ignore]
	fn rebuild_stamp_resign_and_dmg_the_mac_app_end_to_end() {
		let app_template = rebuild_mac_universal_app().expect("universalビルドに失敗");
		assert!(app_template.exists());

		let dir = std::env::temp_dir().join(format!("shelf_gen_test_e2e_mac_{}", std::process::id()));
		let out_dmg = dir.join("TestLauncher.dmg");
		let config_json = r#"{"schema":1,"launcherName":"テスト","manifestUrl":"file:///tmp/manifest.json","manifestPublicKey":"","manualUrl":"","apps":[]}"#;

		stamp_and_resign_mac_app(&app_template, config_json, "TestLauncher", &out_dmg)
			.expect("stamp_and_resign_mac_appに失敗");
		if let Some(dir) = app_template.parent() { let _ = std::fs::remove_dir_all(dir); }
		assert!(out_dmg.exists());

		// dmgをマウントして中の.appの署名・埋め込み設定を確かめる
		let attach = Command::new("/usr/bin/hdiutil").args(["attach", "-nobrowse", "-readonly"]).arg(&out_dmg).output().unwrap();
		assert!(attach.status.success());
		let text = String::from_utf8_lossy(&attach.stdout);
		let mount_line = text.lines().rev().find(|l| l.contains('\t') && l.contains("/Volumes/")).unwrap();
		let mut fields = mount_line.split('\t').map(str::trim).filter(|s| !s.is_empty());
		let device = fields.next().unwrap().to_string();
		let mount_point = fields.last().unwrap();

		let mounted_app = PathBuf::from(mount_point).join("TestLauncher.app");
		let verify = Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&mounted_app).output().unwrap();
		let config_on_disk = std::fs::read_to_string(mounted_app.join("Contents/Resources/shelf-config.json")).unwrap();

		let _ = Command::new("/usr/bin/hdiutil").args(["detach", &device]).status();
		let _ = std::fs::remove_dir_all(&dir);

		assert!(verify.status.success(), "codesign --verify --deep --strict に失敗: {}", String::from_utf8_lossy(&verify.stderr));
		assert_eq!(config_on_disk, config_json);
	}

	#[test]
	fn find_win_prebuilt_exe_returns_none_when_absent() {
		// 開発機にWin実機ビルドの配置が無い前提のテスト（CI/開発機の既定状態）
		let prebuilt = shelf_root().join("gen/prebuilt/x86_64-pc-windows-msvc/shelf.exe");
		if !prebuilt.exists() {
			assert!(find_win_prebuilt_exe().is_none());
		}
	}
}
