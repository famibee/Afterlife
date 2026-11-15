// 開発用フィクスチャの生成。`shelf/dev-fixtures/` に、SHELF_CONFIG で読ませる設定・
// file:// で取るマニフェストとその署名（捨て鍵）・偽の.appインストールを作る。
// 実ゲームを入れていないmacでも、検出・購入者チェック・状態遷移の全経路を通せるようにする。
//
// 実行: cargo run -p shelf_core --bin gen_dev_fixtures
//
// ⚠️ ここで生成する ed25519 鍵は捨て鍵（シード固定・リポジトリに平文で入る）。
// 本番のマニフェスト署名には絶対に使わない。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use shelf_core::{asar, checksum};
use std::fs;
use std::path::Path;

const FIXED_DEV_SEED: [u8; 32] = [0x42; 32];	// 捨て鍵。本番の鍵と絶対に混同しないこと

fn fixtures_root() -> &'static Path {
	// CARGO_MANIFEST_DIR は shelf/core。1階層上がって shelf/ にする
	Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new("."))
}

fn write(path: &Path, content: &str) {
	fs::create_dir_all(path.parent().unwrap()).unwrap();
	fs::write(path, content).unwrap();
	println!("wrote {}", path.display());
}

fn build_fake_install(root: &Path, app_name: &str, setting_sn_content: &[u8], version: &str) -> String {
	// asarルートに setting.sn と package.json の2ファイルを持つ、実物相当の.appを作る
	let pkg_json = format!(r#"{{"name":"{app_name}","version":"{version}"}}"#);
	let pkg_bytes = pkg_json.as_bytes();

	let mut bodies = Vec::new();
	bodies.extend_from_slice(setting_sn_content);
	bodies.extend_from_slice(pkg_bytes);

	let header = format!(
		r#"{{"files":{{"setting.sn":{{"size":{},"offset":"0"}},"package.json":{{"size":{},"offset":"{}"}}}}}}"#,
		setting_sn_content.len(), pkg_bytes.len(), setting_sn_content.len(),
	);
	let asar_bytes = asar::build_fake_asar(&header, &bodies);

	let resources = root.join(format!("{app_name}.app")).join("Contents/Resources");
	fs::create_dir_all(&resources).unwrap();
	fs::write(resources.join("app.asar"), &asar_bytes).unwrap();

	checksum::checksum_hex(setting_sn_content)
}

/// NeedsConfirmation（setting.sn自体が読めない）シナリオ用：asarはあるがsetting.snを
/// 持たない.appを作る。②フォールバック（当時のインストーラーを選ぶ）の確認に使う
fn build_fake_install_without_setting_sn(root: &Path, app_name: &str, version: &str) {
	let pkg_json = format!(r#"{{"name":"{app_name}","version":"{version}"}}"#);
	let pkg_bytes = pkg_json.as_bytes();
	let header = format!(r#"{{"files":{{"package.json":{{"size":{},"offset":"0"}}}}}}"#, pkg_bytes.len());
	let asar_bytes = asar::build_fake_asar(&header, pkg_bytes);

	let resources = root.join(format!("{app_name}.app")).join("Contents/Resources");
	fs::create_dir_all(&resources).unwrap();
	fs::write(resources.join("app.asar"), &asar_bytes).unwrap();
}

/// ②フォールバック用の「当時のインストーラー」フィクスチャファイルを作り、
/// そのサンプリングチェックサムを返す（configのcheckumInstallerに埋める値）
fn build_fake_legacy_installer(out_dir: &Path, file_name: &str, content: &[u8]) -> String {
	let path = out_dir.join(file_name);
	fs::write(&path, content).unwrap();
	checksum::sampled_file_checksum(&path).unwrap()
}

fn main() {
	let shelf_root = fixtures_root();
	let out_dir = shelf_root.join("dev-fixtures");

	// --- 1. 署名鍵（捨て鍵） ---
	let signing_key = SigningKey::from_bytes(&FIXED_DEV_SEED);
	let public_key_b64 = STANDARD.encode(signing_key.verifying_key().as_bytes());
	write(
		&out_dir.join("README.md"),
		&format!(
			"# dev-fixtures（開発専用・捨て鍵）\n\n\
			ここにある ed25519 鍵・署名はシード固定の捨て鍵で、本番のマニフェスト署名には\n\
			絶対に使わない。`cargo run -p shelf_core --bin gen_dev_fixtures` で作り直せる\n\
			（このファイルを含め全て再生成される）。\n\n\
			公開鍵（base64）: `{public_key_b64}`\n",
		),
	);

	// --- 2. 偽インストール（4状態：更新あり／製品版を確認できません／確認が必要／未インストール） ---
	let install_root = out_dir.join("fake-install");
	let _ = fs::remove_dir_all(&install_root);
	let genuine_setting_sn = b"&const.experience = false; // product build";
	let trial_setting_sn = b"&const.experience = true; // trial build";

	// SampleGame: 自動検出成功（更新あり、の経路。最新版はmanifest側で1.4.0にする）
	let genuine_checksum = build_fake_install(&install_root, "SampleGame", genuine_setting_sn, "1.3.0");

	// TrialLookalike: setting.snは読めるがチェックサム不一致＝NotGenuine
	// （「製品版を確認できません」。起動・購入案内のみ、更新チェックはしない）
	build_fake_install(&install_root, "TrialLookalike", trial_setting_sn, "1.0.0");

	// NeedsConfirmShowcase: asarにsetting.sn自体が無い＝自動検出不能＝NeedsConfirmation
	// （②フォールバック「当時のインストーラーを選ぶ」の確認用）
	build_fake_install_without_setting_sn(&install_root, "NeedsConfirmShowcase", "1.0.0");
	let legacy_installer_checksum = build_fake_legacy_installer(
		&out_dir, "fake-legacy-installer.bin", b"this is the old genuine installer bytes",
	);

	// --- 3. 埋め込み設定（SHELF_CONFIG用） ---
	let manifest_path = out_dir.join("manifest.json");
	let manual_url = "https://github.com/famibee/Afterlife/blob/main/shelf/docs/manual.md";

	let embedded_config = format!(
		r#"{{
  "schema": 1,
  "launcherName": "開発用 Shelf",
  "manifestUrl": "file://{}",
  "manifestPublicKey": "{public_key_b64}",
  "manualUrl": "{manual_url}",
  "apps": [
    {{
      "appId": "sample_game",
      "appName": "SampleGame",
      "settingSnFileName": "setting.sn",
      "checksumSetting": ["{genuine_checksum}"],
      "checksumInstaller": []
    }},
    {{
      "appId": "trial_lookalike",
      "appName": "TrialLookalike",
      "settingSnFileName": "setting.sn",
      "checksumSetting": ["{genuine_checksum}"],
      "checksumInstaller": []
    }},
    {{
      "appId": "needs_confirm_showcase",
      "appName": "NeedsConfirmShowcase",
      "settingSnFileName": "setting.sn",
      "checksumSetting": ["{genuine_checksum}"],
      "checksumInstaller": ["{legacy_installer_checksum}"]
    }},
    {{
      "appId": "not_installed_game",
      "appName": "NotInstalledGame",
      "settingSnFileName": "setting.sn",
      "checksumSetting": [],
      "checksumInstaller": []
    }}
  ]
}}
"#,
		manifest_path.display(),
	);
	write(&out_dir.join("config.json"), &embedded_config);

	// --- 4. 遠隔マニフェスト（最新版情報。file://で読む） ---
	// sample_game: 最新版が1.4.0 = 更新あり
	// trial_lookalike: 最新版は何でもよい（ステータスはNeedsConfirmationで決まる）
	// not_installed_game: インストールされていない作品（下段・灰色の対象）
	let manifest_body = r#"{
  "schema": 1,
  "generatedAt": "2026-11-16T00:00:00Z",
  "circle": {
    "name": "開発用サークル",
    "siteUrl": "https://example.com",
    "supportUrl": "https://example.com/support",
    "email": "dev@example.com",
    "links": []
  },
  "apps": [
    {
      "appId": "sample_game",
      "name": "サンプルゲーム",
      "summary": "動作確認用のダミー作品です。",
      "iconPngBase64": "",
      "latest": {
        "version": "1.4.0",
        "mac": {"arch": "universal", "kind": "dmg", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "size": 0, "url": "file:///dev/null"},
        "win": {"arch": "x64", "kind": "nsis", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "size": 0, "url": "file:///dev/null"}
      },
      "store": {"label": "配布サイト", "url": "https://example.com/sample_game"},
      "eventOnly": false
    },
    {
      "appId": "trial_lookalike",
      "name": "体験版誤認サンプル",
      "summary": "製品版チェックサムが一致しないケースの確認用です。",
      "iconPngBase64": "",
      "latest": {
        "version": "1.0.0",
        "mac": {"arch": "universal", "kind": "dmg", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "size": 0, "url": "file:///dev/null"}
      },
      "store": {"label": "購入ページ", "url": "https://example.com/trial_lookalike"},
      "eventOnly": false
    },
    {
      "appId": "needs_confirm_showcase",
      "name": "確認要サンプル",
      "summary": "setting.sn自体が読めず自動検出できないケースの確認用です。",
      "iconPngBase64": "",
      "latest": {
        "version": "1.0.0",
        "mac": {"arch": "universal", "kind": "dmg", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "size": 0, "url": "file:///dev/null"}
      },
      "store": {"label": "購入ページ", "url": "https://example.com/needs_confirm_showcase"},
      "eventOnly": false
    },
    {
      "appId": "not_installed_game",
      "name": "未インストールの作品",
      "summary": "下段・灰色表示の確認用です。",
      "iconPngBase64": "",
      "latest": {
        "version": "1.0.0",
        "mac": {"arch": "universal", "kind": "dmg", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "size": 0, "url": "file:///dev/null"}
      },
      "store": {"label": "配布サイト", "url": "https://example.com/not_installed_game"},
      "eventOnly": true
    }
  ]
}
"#;
	write(&manifest_path, manifest_body);

	let signature = signing_key.sign(manifest_body.as_bytes());
	let sig_b64 = STANDARD.encode(signature.to_bytes());
	write(&out_dir.join("manifest.json.sig"), &sig_b64);

	println!("\n生成完了。起動方法:");
	println!(
		"  SHELF_MAC_APPS_DIR={}/fake-install SHELF_CONFIG={}/config.json npm run app:dev",
		out_dir.display(), out_dir.display(),
	);
	println!("\nシナリオ: sample_game=更新あり trial_lookalike=製品版を確認できません");
	println!("          needs_confirm_showcase=確認が必要（fake-legacy-installer.binを選ぶと一致） not_installed_game=未インストール");
}
