// 遠隔マニフェスト（manifest.json）の取得・署名検証・パース。
//
// URLを隠す代わりに内容の正しさを検証する（秘匿の段階0/1とは別の話：段階を問わず、
// 自動インストールは取得物を実行するので、マニフェスト自体が差し替えられたら任意の
// URL・チェックサムを注入される。ed25519署名で「生成ツールが作ったもの」であることを
// 確かめてから信じる）。
//
// curlをサブプロセスで呼ぶだけ（download.rs・旧manifest.rsと同じ考え方。reqwest等の
// HTTPクライアントクレートは追加しない。暗号の原始関数〈sha2・ed25519-dalek〉は
// 自前実装のリスクを避けるためクレートに任せる＝依存を増やさない方針とは別の判断軸）。
//
// マニフェストの実体は `manifest.json`、署名は同じ場所の `manifest.json.sig`
// （本文バイト列そのものに対するed25519署名、base64エンコード）に分けて置く。
// 生成ツール側がアップロード時にこの2本を作る（第2段階で実装）。

use serde::Deserialize;
use std::io;
use std::path::Path;
use std::process::Command;

#[cfg(target_os = "windows")]
const CURL: &str = r"C:\Windows\System32\curl.exe";
#[cfg(target_os = "macos")]
const CURL: &str = "/usr/bin/curl";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OsArtifact {
	pub arch: String,	// "x64"|"ia32"|"arm64"|"universal"
	pub kind: String,	// "nsis"|"dmg" 等（表示用。判定には使わない）
	pub sha256: String,
	pub size: u64,
	pub url: String,
	/// 段階1（Cloudflare Workerによる所持証明付き短命URL）への差し替え口。
	/// Noneなら段階0としてurlをそのまま使う。実装は第1段階の対象外（計画参照）
	#[serde(default)]
	pub grant_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LatestRelease {
	pub version: String,
	#[serde(default)]
	pub mac: Option<OsArtifact>,
	#[serde(default)]
	pub win: Option<OsArtifact>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoreInfo {
	pub label: String,
	pub url: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestApp {
	pub app_id: String,
	pub name: String,
	#[serde(default)]
	pub summary: String,
	#[serde(default)]
	pub icon_png_base64: String,
	pub latest: LatestRelease,
	#[serde(default)]
	pub store: Option<StoreInfo>,
	#[serde(default)]
	pub event_only: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CircleInfo {
	pub name: String,
	#[serde(default)]
	pub site_url: String,
	#[serde(default)]
	pub support_url: String,
	#[serde(default)]
	pub email: String,
	#[serde(default)]
	pub links: Vec<CircleLink>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CircleLink {
	pub label: String,
	pub url: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
	pub schema: u32,
	#[serde(default)]
	pub generated_at: String,
	pub circle: CircleInfo,
	pub apps: Vec<ManifestApp>,
}

impl Manifest {
	pub fn app(&self, app_id: &str) -> Option<&ManifestApp> {
		self.apps.iter().find(|a| a.app_id == app_id)
	}
}

impl ManifestApp {
	pub fn for_platform(&self, platform: crate::detect::Platform) -> Option<&OsArtifact> {
		match platform {
			crate::detect::Platform::Mac => self.latest.mac.as_ref(),
			crate::detect::Platform::Windows => self.latest.win.as_ref(),
			crate::detect::Platform::Other => None,
		}
	}
}

#[derive(Debug)]
pub enum ManifestError {
	Fetch(String),
	Signature(String),
	Parse(serde_json::Error),
}

impl std::fmt::Display for ManifestError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Fetch(e) => write!(f, "マニフェストの取得に失敗: {e}"),
			Self::Signature(e) => write!(f, "マニフェストの署名検証に失敗: {e}"),
			Self::Parse(e) => write!(f, "マニフェストの解析に失敗: {e}"),
		}
	}
}

fn curl_get(url: &str) -> io::Result<Vec<u8>> {
	let output = Command::new(CURL)
		.arg("-fsSL")	// 失敗時に非ゼロ終了・サイレント・リダイレクト追従
		.arg(url)
		.output()?;
	if !output.status.success() {
		return Err(io::Error::other(format!("curl が失敗した（終了コード: {}）", output.status)));
	}
	Ok(output.stdout)
}

/// base64（標準アルファベット）の ed25519 公開鍵で、本文バイト列に対する署名を検証する。
/// 署名はbase64エンコードされた64バイトの生署名。
pub fn verify_signature(public_key_b64: &str, body: &[u8], signature_b64: &str) -> Result<(), String> {
	use base64::{engine::general_purpose::STANDARD, Engine as _};
	use ed25519_dalek::{Signature, VerifyingKey};

	let key_bytes = STANDARD.decode(public_key_b64.trim())
		.map_err(|e| format!("公開鍵のbase64デコードに失敗: {e}"))?;
	let key_bytes: [u8; 32] = key_bytes.try_into()
		.map_err(|_| "公開鍵は32バイトである必要がある".to_string())?;
	let verifying_key = VerifyingKey::from_bytes(&key_bytes)
		.map_err(|e| format!("公開鍵の形式が不正: {e}"))?;

	let sig_bytes = STANDARD.decode(signature_b64.trim())
		.map_err(|e| format!("署名のbase64デコードに失敗: {e}"))?;
	let sig_bytes: [u8; 64] = sig_bytes.try_into()
		.map_err(|_| "署名は64バイトである必要がある".to_string())?;
	let signature = Signature::from_bytes(&sig_bytes);

	verifying_key.verify_strict(body, &signature)
		.map_err(|e| format!("署名が一致しない: {e}"))
}

fn parse_body(body: &[u8]) -> Result<Manifest, ManifestError> {
	serde_json::from_slice(body).map_err(ManifestError::Parse)
}

/// マニフェスト本体と署名を取得し、検証してからパースする。
/// `manifest_url` が `foo/manifest.json` なら、署名は `foo/manifest.json.sig` から取る。
pub fn fetch_and_verify(manifest_url: &str, public_key_b64: &str) -> Result<(Manifest, Vec<u8>), ManifestError> {
	let body = curl_get(manifest_url).map_err(|e| ManifestError::Fetch(e.to_string()))?;
	let sig_url = format!("{manifest_url}.sig");
	let sig_bytes = curl_get(&sig_url).map_err(|e| ManifestError::Fetch(format!("署名の取得に失敗: {e}")))?;
	let sig_b64 = String::from_utf8_lossy(&sig_bytes).trim().to_string();

	verify_signature(public_key_b64, &body, &sig_b64).map_err(ManifestError::Signature)?;

	let manifest = parse_body(&body)?;
	Ok((manifest, body))
}

/// キャッシュから読む（起動時に即描くため。署名はキャッシュ時点で検証済みの前提で
/// 再検証しない＝ローカルファイルシステム自体への信頼に委ねる。他プロセスからの
/// 改ざんは脅威モデル外）
pub fn load_cached(cache_path: &Path) -> Option<Manifest> {
	let bytes = std::fs::read(cache_path).ok()?;
	parse_body(&bytes).ok()
}

pub fn save_cache(cache_path: &Path, raw_json: &[u8]) -> io::Result<()> {
	if let Some(parent) = cache_path.parent() {
		std::fs::create_dir_all(parent)?;
	}
	std::fs::write(cache_path, raw_json)
}

#[cfg(test)]
mod tests {
	use super::*;
	use ed25519_dalek::{Signer, SigningKey};

	fn sample_manifest_json() -> &'static str {
		r#"{"schema":1,"generatedAt":"2026-11-16T00:00:00Z",
			"circle":{"name":"ふぁみべえ","siteUrl":"https://example.com","supportUrl":"","email":"","links":[]},
			"apps":[{"appId":"sn_kowloon","name":"大阪九龍条","summary":"","iconPngBase64":"",
				"latest":{"version":"1.3.0",
					"mac":{"arch":"universal","kind":"dmg","sha256":"abc","size":100,"url":"https://example.com/m.dmg"},
					"win":{"arch":"x64","kind":"nsis","sha256":"def","size":200,"url":"https://example.com/w.exe"}},
				"store":{"label":"DLsite","url":"https://example.com/store"},"eventOnly":false}]}"#
	}

	fn write_temp(content: &[u8], suffix: &str) -> std::path::PathBuf {
		let path = std::env::temp_dir().join(format!("shelf_manifest_test_{}_{suffix}", std::process::id()));
		std::fs::write(&path, content).unwrap();
		path
	}

	#[test]
	fn parse_body_reads_all_fields() {
		let manifest = parse_body(sample_manifest_json().as_bytes()).unwrap();
		assert_eq!(manifest.schema, 1);
		assert_eq!(manifest.circle.name, "ふぁみべえ");
		let app = manifest.app("sn_kowloon").unwrap();
		assert_eq!(app.name, "大阪九龍条");
		assert_eq!(app.latest.version, "1.3.0");
		assert_eq!(app.latest.mac.as_ref().unwrap().arch, "universal");
		assert!(app.latest.mac.as_ref().unwrap().grant_url.is_none());
	}

	#[test]
	fn app_lookup_returns_none_for_unknown_id() {
		let manifest = parse_body(sample_manifest_json().as_bytes()).unwrap();
		assert!(manifest.app("no_such_app").is_none());
	}

	#[test]
	fn verify_signature_accepts_valid_signature_and_rejects_tampered_body() {
		use base64::{engine::general_purpose::STANDARD, Engine as _};

		let signing_key = SigningKey::from_bytes(&[7u8; 32]);
		let public_key_b64 = STANDARD.encode(signing_key.verifying_key().as_bytes());

		let body = b"hello manifest body";
		let signature = signing_key.sign(body);
		let sig_b64 = STANDARD.encode(signature.to_bytes());

		assert!(verify_signature(&public_key_b64, body, &sig_b64).is_ok());
		assert!(verify_signature(&public_key_b64, b"tampered body", &sig_b64).is_err());
	}

	#[test]
	fn verify_signature_rejects_malformed_base64() {
		assert!(verify_signature("not-base64-key", b"x", "not-base64-sig").is_err());
	}

	#[test]
	fn fetch_and_verify_reads_local_file_via_curl_when_signature_matches() {
		use base64::{engine::general_purpose::STANDARD, Engine as _};
		use ed25519_dalek::Signer;

		let signing_key = SigningKey::from_bytes(&[9u8; 32]);
		let public_key_b64 = STANDARD.encode(signing_key.verifying_key().as_bytes());

		let body = sample_manifest_json().as_bytes();
		let signature = signing_key.sign(body);
		let sig_b64 = STANDARD.encode(signature.to_bytes());

		let manifest_path = write_temp(body, "manifest_ok.json");
		let sig_path = manifest_path.with_extension("json.sig");
		std::fs::write(&sig_path, sig_b64).unwrap();

		let url = format!("file://{}", manifest_path.display());
		let (manifest, _raw) = fetch_and_verify(&url, &public_key_b64).unwrap();
		assert_eq!(manifest.app("sn_kowloon").unwrap().latest.version, "1.3.0");

		std::fs::remove_file(&manifest_path).unwrap();
		std::fs::remove_file(&sig_path).unwrap();
	}

	#[test]
	fn fetch_and_verify_fails_when_signature_is_for_different_key() {
		use base64::{engine::general_purpose::STANDARD, Engine as _};
		use ed25519_dalek::Signer;

		let signing_key = SigningKey::from_bytes(&[1u8; 32]);
		let wrong_key = SigningKey::from_bytes(&[2u8; 32]);
		let public_key_b64 = STANDARD.encode(wrong_key.verifying_key().as_bytes());

		let body = sample_manifest_json().as_bytes();
		let signature = signing_key.sign(body);
		let sig_b64 = STANDARD.encode(signature.to_bytes());

		let manifest_path = write_temp(body, "manifest_bad.json");
		let sig_path = manifest_path.with_extension("json.sig");
		std::fs::write(&sig_path, sig_b64).unwrap();

		let url = format!("file://{}", manifest_path.display());
		let result = fetch_and_verify(&url, &public_key_b64);
		assert!(matches!(result, Err(ManifestError::Signature(_))));

		std::fs::remove_file(&manifest_path).unwrap();
		std::fs::remove_file(&sig_path).unwrap();
	}

	#[test]
	fn cache_roundtrips_through_save_and_load() {
		let dir = std::env::temp_dir().join(format!("shelf_manifest_cache_test_{}", std::process::id()));
		let cache_path = dir.join("manifest-cache.json");

		save_cache(&cache_path, sample_manifest_json().as_bytes()).unwrap();
		let loaded = load_cached(&cache_path).unwrap();
		assert_eq!(loaded.app("sn_kowloon").unwrap().name, "大阪九龍条");

		std::fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn load_cached_returns_none_when_file_is_missing() {
		assert!(load_cached(Path::new("/no/such/cache.json")).is_none());
	}
}
