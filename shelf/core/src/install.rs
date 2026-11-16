// 取得→検証→インストールの一連の流れ。
//
// 自動インストールの手順は実機検証済み（2026-11-16）：
// - mac: hdiutil attach -nobrowse -readonly → 対象.appをditto → 旧版を退避→削除 → detach。
//   「アプリ管理」権限のダイアログは出ず成功した（M6実機）。ただし検証はターミナル/VS Code
//   拡張という既に権限が通っている起動元からのもので、Shelf自身（未許可の起動元）からの
//   場合に同じ結果になるかは未確認
// - win: `インストーラー.exe /S` はper-userインストールではUAC無し・Avast無反応で成功した
//   （実機）。perMachineインストールでのUAC挙動は未検証
//
// 検証に失敗した場合・自動インストールの手順自体が失敗した場合は、「取得して開くだけ」
// （download.rsの既存動作）に退避する。ハッシュ不一致は完全性の問題なので退避せず
// エラーのまま止める（取得物を実行しないようにする）。

use crate::{checksum, download, manifest::OsArtifact};
use std::io;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
	/// 自動インストールが完了した
	Installed,
	/// 自動インストールに失敗・権限等に止められたため、取得しただけで開いて利用者に委ねた
	DownloadedOnly(PathBuf),
}

#[derive(Debug)]
pub enum InstallError {
	Download(String),
	ChecksumMismatch { expected: String, actual: String },
}

impl std::fmt::Display for InstallError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Download(e) => write!(f, "取得に失敗: {e}"),
			Self::ChecksumMismatch { expected, actual } =>
				write!(f, "取得したファイルのハッシュが一致しない（期待: {expected}, 実際: {actual}）"),
		}
	}
}

#[cfg(target_os = "macos")]
fn hdiutil_attach(dmg_path: &Path) -> io::Result<(String, PathBuf)> {
	let output = Command::new("/usr/bin/hdiutil")
		.args(["attach", "-nobrowse", "-readonly"])
		.arg(dmg_path)
		.output()?;
	if !output.status.success() {
		return Err(io::Error::other(format!("hdiutil attach が失敗した: {}", String::from_utf8_lossy(&output.stderr))));
	}
	let text = String::from_utf8_lossy(&output.stdout);
	let mount_line = text.lines().rev().find(|l| l.contains('\t') && l.contains("/Volumes/"))
		.ok_or_else(|| io::Error::other("マウントポイントの行が見つからない"))?;
	let mut fields = mount_line.split('\t').map(str::trim).filter(|s| !s.is_empty());
	let device = fields.next().ok_or_else(|| io::Error::other("デバイスノードが見つからない"))?.to_string();
	let mount_point = fields.last().ok_or_else(|| io::Error::other("マウントポイントが見つからない"))?.to_string();
	Ok((device, PathBuf::from(mount_point)))
}

#[cfg(target_os = "macos")]
fn hdiutil_detach(device: &str) {
	let _ = Command::new("/usr/bin/hdiutil").args(["detach", device]).status();
}

#[cfg(target_os = "macos")]
fn find_app_bundle(dir: &Path) -> io::Result<PathBuf> {
	for entry in std::fs::read_dir(dir)? {
		let path = entry?.path();
		if path.extension().and_then(|e| e.to_str()) == Some("app") {
			return Ok(path);
		}
	}
	Err(io::Error::other("dmg内に.appが見つからない"))
}

#[cfg(target_os = "macos")]
fn mac_swap_install(mounted_app: &Path, app_name: &str, apps_dir: &Path) -> io::Result<()> {
	let dest = apps_dir.join(format!("{app_name}.app"));
	let old = apps_dir.join(format!("{app_name}.app.old"));
	let _ = std::fs::remove_dir_all(&old);	// 前回の残骸があれば先に消す

	let had_old_install = dest.exists();
	if had_old_install {
		std::fs::rename(&dest, &old)?;
	}

	let ditto = Command::new("/usr/bin/ditto").arg(mounted_app).arg(&dest).status()?;
	if !ditto.success() {
		if had_old_install {
			let _ = std::fs::rename(&old, &dest);	// 失敗したら退避した旧版を戻す
		}
		return Err(io::Error::other("ditto によるコピーに失敗した"));
	}

	let _ = std::fs::remove_dir_all(&old);
	Ok(())
}

#[cfg(target_os = "macos")]
fn install_auto(downloaded_path: &Path, app_name: &str, apps_dir: &Path) -> io::Result<()> {
	let (device, mount_point) = hdiutil_attach(downloaded_path)?;
	let result = (|| {
		let app = find_app_bundle(&mount_point)?;
		mac_swap_install(&app, app_name, apps_dir)
	})();
	hdiutil_detach(&device);
	result
}

// `requireAdministrator`のインストーラーをCreateProcess（Command::new）で非昇格のまま起動すると、
// UACダイアログが出ないままERROR_ELEVATION_REQUIRED(os error 740)で失敗する（2026-11-17・Win実機で
// perMachineインストーラーにて確認）。ShellExecuteExWに"runas"動詞で昇格を要求する必要がある。
#[cfg(target_os = "windows")]
fn install_auto(downloaded_path: &Path, _app_name: &str, _apps_dir: &Path) -> io::Result<()> {
	use std::os::windows::ffi::OsStrExt;
	use windows::Win32::Foundation::CloseHandle;
	use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
	use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
	use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
	use windows::core::PCWSTR;

	fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
		s.encode_wide().chain(std::iter::once(0)).collect()
	}

	let file = wide(downloaded_path.as_os_str());
	let params = wide(std::ffi::OsStr::new("/S"));
	let verb = wide(std::ffi::OsStr::new("runas"));

	let mut info = SHELLEXECUTEINFOW {
		cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
		fMask: SEE_MASK_NOCLOSEPROCESS,
		lpVerb: PCWSTR(verb.as_ptr()),
		lpFile: PCWSTR(file.as_ptr()),
		lpParameters: PCWSTR(params.as_ptr()),
		nShow: SW_SHOWNORMAL.0,
		..Default::default()
	};

	unsafe {
		ShellExecuteExW(&mut info).map_err(|e| {
			// ユーザーがUACダイアログを拒否した場合はERROR_CANCELLED(1223)になる
			io::Error::other(format!("インストーラーの起動に失敗した（昇格を拒否した可能性）: {e}"))
		})?;

		if info.hProcess.is_invalid() {
			return Err(io::Error::other("インストーラーのプロセスハンドルを取得できなかった"));
		}
		WaitForSingleObject(info.hProcess, INFINITE);
		let mut exit_code = 0u32;
		let result = GetExitCodeProcess(info.hProcess, &mut exit_code);
		let _ = CloseHandle(info.hProcess);
		result.map_err(|e| io::Error::other(format!("終了コードの取得に失敗した: {e}")))?;

		if exit_code == 0 { Ok(()) }
		else { Err(io::Error::other(format!("インストーラーが失敗した（終了コード: {exit_code}）"))) }
	}
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn install_auto(_downloaded_path: &Path, _app_name: &str, _apps_dir: &Path) -> io::Result<()> {
	Err(io::Error::other("対象外プラットフォーム"))
}

/// `dest_dir` は取得先（一時ディレクトリ）。`app_name` はmacのインストール先
/// （`<apps_dir>/<app_name>.app`）を決めるのに使う。`apps_dir` は本番では`/Applications`
/// だが、検証時は呼び出し側（lib.rsの`mac_apps_dir()`）がSHELF_MAC_APPS_DIRで
/// 差し替えられる（Windowsでは使わない）。
pub fn install_update(artifact: &OsArtifact, app_name: &str, dest_dir: &Path, apps_dir: &Path) -> Result<InstallOutcome, InstallError> {
	let path = download::download(&artifact.url, dest_dir)
		.map_err(|e| InstallError::Download(e.to_string()))?;

	let actual = checksum::file_checksum(&path)
		.map_err(|e| InstallError::Download(format!("検証用ハッシュの計算に失敗: {e}")))?;
	if !actual.eq_ignore_ascii_case(&artifact.sha256) {
		return Err(InstallError::ChecksumMismatch { expected: artifact.sha256.clone(), actual });
	}

	match install_auto(&path, app_name, apps_dir) {
		Ok(()) => Ok(InstallOutcome::Installed),
		Err(_e) => {
			// 自動インストールに失敗・権限等に止められた場合は「取得して開くだけ」に退避する
			let _ = download::open_downloaded_file(&path);
			Ok(InstallOutcome::DownloadedOnly(path))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn sample_artifact(sha256: &str, url: &str) -> OsArtifact {
		OsArtifact {
			arch: "universal".to_string(),
			kind: "dmg".to_string(),
			sha256: sha256.to_string(),
			size: 0,
			url: url.to_string(),
			grant_url: None,
		}
	}

	#[test]
	fn install_update_fails_fast_on_checksum_mismatch_without_running_installer() {
		let dir = std::env::temp_dir().join(format!("shelf_install_test_{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let fake_download = dir.join("fake-installer.bin");
		std::fs::write(&fake_download, b"not the real content").unwrap();

		let url = format!("file://{}", fake_download.display());
		let artifact = sample_artifact("0000000000000000000000000000000000000000000000000000000000000000", &url);

		let result = install_update(&artifact, "NoSuchApp", &dir, &PathBuf::from("/Applications"));
		assert!(matches!(result, Err(InstallError::ChecksumMismatch { .. })));

		std::fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn install_update_fails_when_source_url_is_unreachable() {
		let dir = std::env::temp_dir().join(format!("shelf_install_test_unreachable_{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();

		let artifact = sample_artifact("anything", "file:///no/such/path/installer.bin");
		let result = install_update(&artifact, "NoSuchApp", &dir, &PathBuf::from("/Applications"));
		assert!(matches!(result, Err(InstallError::Download(_))));

		std::fs::remove_dir_all(&dir).unwrap();
	}
}
