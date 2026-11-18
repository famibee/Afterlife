// カードの状態遷移を決める純関数。legacy-app-patch.md「カードの状態遷移」の表を踏襲しつつ、
// 体験版誤認（製品版を確認できません）をNotGenuineとして明示的に分けた点が旧実装との違い
// （旧実装はチェックサム不一致＝NotInstalled〈カード非表示〉だったが、Shelfでは
// 「インストールはされているが製品版と確認できない」ことを利用者に伝え、購入を案内する）。

use crate::{detect::Arch, inspect::LocalState, manifest::{ManifestApp, OsArtifact}, version};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardStatus {
	/// マニフェスト取得・ローカル検査のいずれかが未完了
	Checking,
	/// 検出あり・版一致・arch一致
	UpToDate,
	/// 検出あり・版が古い
	UpdateAvailable,
	/// 検出あり・版は同じだがarchが一致しない（例：旧x64購入者にuniversal版への移行を促す）
	ArchMigration,
	/// 検出あり・マニフェスト上は新しい版があるが、このOS向けの配布物が無い
	/// （片方のOSだけ新版を出した場合。更新ボタンは出さない）
	NoUpdateForOs,
	/// 検出はあるがチェックサム不一致（体験版・改変等）
	NotGenuine,
	/// 自動検出に失敗したが、インストーラー本体によるフォールバック判定が使える
	NeedsConfirmation,
	/// 未インストール（下段・灰色表示の対象）。eventOnlyならイベント配布のみの表記にする
	NotInstalled { event_only: bool },
	/// マニフェストの取得・署名検証に失敗した。ローカル検査の結果だけで出せる範囲で表示する
	ManifestError { locally_installed: bool },
}

/// このOS向けの配布物 `artifact`（無ければNone）まで加味した判定。
/// 配布物が無いのに「更新あり」になる場合は、押してもエラーになるだけなので NoUpdateForOs にする
pub fn decide_status_for_os(
	local: &LocalState,
	manifest_app: Option<&ManifestApp>,
	artifact: Option<&OsArtifact>,
) -> CardStatus {
	let status = decide_status(local, manifest_app, artifact.map(|a| a.arch.as_str()));
	match (&status, artifact) {
		(CardStatus::UpdateAvailable | CardStatus::ArchMigration, None) => CardStatus::NoUpdateForOs,
		_ => status,
	}
}

/// `local` はローカル検査の結果、`manifest_app` は対応するマニフェスト項目
/// （取得・検証に失敗していれば None）、`for_platform` はそのOS向けの配布情報。
pub fn decide_status(
	local: &LocalState,
	manifest_app: Option<&ManifestApp>,
	artifact_arch: Option<&str>,
) -> CardStatus {
	// 自動検出に失敗していても②フォールバック（当時のインストーラーを選ばせる）は
	// マニフェストの成否と無関係に使えるので、マニフェスト欠落チェックより先に判定する
	if matches!(local, LocalState::NeedsConfirmation) {
		return CardStatus::NeedsConfirmation;
	}

	let Some(app) = manifest_app else {
		let locally_installed = matches!(local, LocalState::Installed { .. } | LocalState::NotGenuine { .. });
		return CardStatus::ManifestError { locally_installed };
	};

	match local {
		LocalState::NotInstalled => CardStatus::NotInstalled { event_only: app.event_only },
		LocalState::NeedsConfirmation => unreachable!("上で処理済み"),
		// 体験版等と確認された場合は版・archの突き合わせをしない（起動・購入案内のみ）
		LocalState::NotGenuine { .. } => CardStatus::NotGenuine,
		LocalState::Installed { arch, version: installed_version, .. } => {
			let Some(installed_version) = installed_version else {
				// 版が読めない場合は安全側（更新ありとみなして案内する）
				return CardStatus::UpdateAvailable;
			};

			if version::is_newer(&app.latest.version, installed_version) {
				return CardStatus::UpdateAvailable;
			}
			if !version::is_same(&app.latest.version, installed_version) {
				// リモートがローカルより古い（降格）：異常系だが、更新ボタンは出さない
				return CardStatus::UpToDate;
			}

			match (arch, artifact_arch) {
				(Some(installed_arch), Some(latest_arch)) => {
					if arch_matches(latest_arch, *installed_arch) { CardStatus::UpToDate }
					else { CardStatus::ArchMigration }
				}
				// archが判定できない場合は従来通りスキップを許可する保守的な設計
				// （legacy-app-patch.md 詰められていない仕様#9の後方互換方針を踏襲）
				_ => CardStatus::UpToDate,
			}
		}
	}
}

fn arch_matches(latest_arch: &str, installed: Arch) -> bool {
	if installed == Arch::Universal { return true; }
	match latest_arch.to_lowercase().as_str() {
		"x64" => installed == Arch::X64,
		"ia32" => installed == Arch::Ia32,
		"arm64" => installed == Arch::Arm64,
		_ => true,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::manifest::{LatestRelease, ManifestApp};

	fn app_with_version(version: &str) -> ManifestApp {
		ManifestApp {
			app_id: "a".to_string(),
			name: "App".to_string(),
			summary: String::new(),
			icon_png_base64: String::new(),
			latest: LatestRelease {
				version: version.to_string(),
				mac: None,
				win: None,
			},
			store: None,
			event_only: false,
		}
	}

	fn installed(version: Option<&str>, arch: Option<Arch>) -> LocalState {
		LocalState::Installed { checksum: "x".to_string(), arch, version: version.map(str::to_string) }
	}

	#[test]
	fn not_installed_maps_to_not_installed_with_event_only_flag() {
		let mut app = app_with_version("1.0.0");
		app.event_only = true;
		let status = decide_status(&LocalState::NotInstalled, Some(&app), None);
		assert_eq!(status, CardStatus::NotInstalled { event_only: true });
	}

	#[test]
	fn needs_confirmation_passes_through_regardless_of_manifest() {
		let app = app_with_version("1.0.0");
		assert_eq!(decide_status(&LocalState::NeedsConfirmation, Some(&app), None), CardStatus::NeedsConfirmation);
		assert_eq!(decide_status(&LocalState::NeedsConfirmation, None, None), CardStatus::NeedsConfirmation);
	}

	#[test]
	fn up_to_date_when_versions_match_and_arch_unspecified() {
		let app = app_with_version("1.3.0");
		let local = installed(Some("1.3.0"), None);
		assert_eq!(decide_status(&local, Some(&app), None), CardStatus::UpToDate);
	}

	#[test]
	fn update_available_when_remote_version_is_newer() {
		let app = app_with_version("1.4.0");
		let local = installed(Some("1.3.0"), None);
		assert_eq!(decide_status(&local, Some(&app), None), CardStatus::UpdateAvailable);
	}

	#[test]
	fn update_available_when_installed_version_is_unknown() {
		let app = app_with_version("1.4.0");
		let local = installed(None, None);
		assert_eq!(decide_status(&local, Some(&app), None), CardStatus::UpdateAvailable);
	}

	#[test]
	fn arch_migration_when_version_same_but_arch_differs() {
		let app = app_with_version("1.3.0");
		let local = installed(Some("1.3.0"), Some(Arch::X64));
		assert_eq!(decide_status(&local, Some(&app), Some("arm64")), CardStatus::ArchMigration);
	}

	#[test]
	fn up_to_date_when_installed_arch_is_universal() {
		let app = app_with_version("1.3.0");
		let local = installed(Some("1.3.0"), Some(Arch::Universal));
		assert_eq!(decide_status(&local, Some(&app), Some("x64")), CardStatus::UpToDate);
	}

	#[test]
	fn not_genuine_ignores_version_and_arch_comparison() {
		let app = app_with_version("1.0.0");
		let local = LocalState::NotGenuine { checksum: "x".to_string(), arch: Some(Arch::X64), version: Some("9.9.9".to_string()) };
		assert_eq!(decide_status(&local, Some(&app), Some("arm64")), CardStatus::NotGenuine);
	}

	#[test]
	fn manifest_error_reports_whether_locally_installed() {
		let installed_local = installed(Some("1.0.0"), None);
		assert_eq!(decide_status(&installed_local, None, None), CardStatus::ManifestError { locally_installed: true });
		assert_eq!(decide_status(&LocalState::NotInstalled, None, None), CardStatus::ManifestError { locally_installed: false });
	}

	fn artifact(arch: &str) -> OsArtifact {
		OsArtifact { arch: arch.to_string(), kind: "dmg".to_string(), sha256: String::new(), size: 0, url: String::new(), grant_url: None }
	}

	#[test]
	fn newer_version_without_artifact_for_this_os_is_no_update_for_os() {
		let app = app_with_version("2.0.0");
		let local = installed(Some("1.0.0"), None);
		assert_eq!(decide_status_for_os(&local, Some(&app), None), CardStatus::NoUpdateForOs);
		// 版が読めない場合も、配布物が無ければ更新を案内しない
		assert_eq!(decide_status_for_os(&installed(None, None), Some(&app), None), CardStatus::NoUpdateForOs);
	}

	#[test]
	fn newer_version_with_artifact_is_still_update_available() {
		let app = app_with_version("2.0.0");
		let local = installed(Some("1.0.0"), None);
		assert_eq!(decide_status_for_os(&local, Some(&app), Some(&artifact("x64"))), CardStatus::UpdateAvailable);
	}

	#[test]
	fn same_version_without_artifact_stays_up_to_date() {
		let app = app_with_version("1.0.0");
		assert_eq!(decide_status_for_os(&installed(Some("1.0.0"), None), Some(&app), None), CardStatus::UpToDate);
	}

	#[test]
	fn not_installed_without_artifact_is_unchanged() {
		let app = app_with_version("2.0.0");
		assert_eq!(decide_status_for_os(&LocalState::NotInstalled, Some(&app), None), CardStatus::NotInstalled { event_only: false });
	}
}
