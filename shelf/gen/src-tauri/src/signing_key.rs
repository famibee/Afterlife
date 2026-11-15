// マニフェスト署名用の ed25519 鍵の生成・保存・読込。
//
// ⚠️ リポジトリ外（ホームディレクトリ配下）に保存する（2026-11-16決定）。Afterlifeは
// 公開リポジトリのため、誤ってcommitする事故を構造的に防ぐ。この鍵を失うと、
// 既に配布済みのランチャー（公開鍵が埋め込み済み）は新しいマニフェストを一切
// 検証できなくなる＝全ランチャーの作り直しが必要になる重要な秘密。
// そのため「既存鍵の読込」と「新規生成」は常に別経路にし、既存鍵を誤って
// 上書きする操作は用意しない（UIからの「作り直し」ボタンも意図的に置かない）。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
struct StoredKey {
	#[serde(rename = "seedB64")]
	seed_b64: String,
	#[serde(rename = "publicKeyB64")]
	public_key_b64: String,
}

pub struct SigningKeyInfo {
	pub signing_key: SigningKey,
	pub public_key_b64: String,
}

pub fn key_path() -> io::Result<PathBuf> {
	let home = std::env::var_os("HOME")
		.ok_or_else(|| io::Error::other("HOME環境変数が見つからない"))?;
	Ok(PathBuf::from(home).join(".shelf-gen").join("signing-key.json"))
}

fn load(path: &std::path::Path) -> io::Result<SigningKeyInfo> {
	let json = std::fs::read_to_string(path)?;
	let stored: StoredKey = serde_json::from_str(&json)
		.map_err(|e| io::Error::other(format!("鍵ファイルの解析に失敗: {e}")))?;
	let seed_bytes = STANDARD.decode(&stored.seed_b64)
		.map_err(|e| io::Error::other(format!("鍵のbase64デコードに失敗: {e}")))?;
	let seed: [u8; 32] = seed_bytes.try_into()
		.map_err(|_| io::Error::other("鍵の長さが不正（32バイトのseedである必要がある）"))?;
	let signing_key = SigningKey::from_bytes(&seed);
	Ok(SigningKeyInfo { signing_key, public_key_b64: stored.public_key_b64 })
}

fn generate_and_save(path: &std::path::Path) -> io::Result<SigningKeyInfo> {
	use rand::rngs::OsRng;

	let signing_key = SigningKey::generate(&mut OsRng);
	let public_key_b64 = STANDARD.encode(signing_key.verifying_key().as_bytes());
	let seed_b64 = STANDARD.encode(signing_key.to_bytes());

	if let Some(dir) = path.parent() {
		std::fs::create_dir_all(dir)?;
	}
	let json = serde_json::to_string_pretty(&StoredKey { seed_b64, public_key_b64: public_key_b64.clone() })
		.map_err(|e| io::Error::other(e.to_string()))?;
	std::fs::write(path, json)?;

	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
	}

	Ok(SigningKeyInfo { signing_key, public_key_b64 })
}

/// 既存の鍵があれば読むだけ（絶対に上書きしない）。無ければ新規生成して保存する。
pub fn load_or_generate() -> io::Result<SigningKeyInfo> {
	let path = key_path()?;
	if path.exists() {
		load(&path)
	}
	else {
		generate_and_save(&path)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn generate_and_save_then_load_roundtrips_the_same_key() {
		let dir = std::env::temp_dir().join(format!("shelf_gen_test_signing_key_{}", std::process::id()));
		let path = dir.join("signing-key.json");
		std::fs::create_dir_all(&dir).unwrap();

		let generated = generate_and_save(&path).unwrap();
		let loaded = load(&path).unwrap();

		assert_eq!(generated.public_key_b64, loaded.public_key_b64);
		assert_eq!(generated.signing_key.to_bytes(), loaded.signing_key.to_bytes());

		std::fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn generate_and_save_sets_owner_only_permissions_on_unix() {
		let dir = std::env::temp_dir().join(format!("shelf_gen_test_signing_key_perm_{}", std::process::id()));
		let path = dir.join("signing-key.json");
		std::fs::create_dir_all(&dir).unwrap();

		generate_and_save(&path).unwrap();

		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt;
			let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
			assert_eq!(mode, 0o600);
		}

		std::fs::remove_dir_all(&dir).unwrap();
	}

	#[test]
	fn load_does_not_regenerate_if_key_already_exists() {
		let dir = std::env::temp_dir().join(format!("shelf_gen_test_signing_key_noclobber_{}", std::process::id()));
		let path = dir.join("signing-key.json");
		std::fs::create_dir_all(&dir).unwrap();

		let first = generate_and_save(&path).unwrap();
		// 2回目はload()（generate_and_saveではない）を通すのがload_or_generateの契約。
		// ここではload単体で同じ鍵が返ることを確認する
		let second = load(&path).unwrap();
		assert_eq!(first.public_key_b64, second.public_key_b64);

		std::fs::remove_dir_all(&dir).unwrap();
	}
}
