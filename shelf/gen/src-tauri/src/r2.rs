// Cloudflare R2（S3互換）へのアップロード・一覧・削除。
// 旧patch_gen_gui（famibee/bluesnovel-vsce コミット41c9c1b）のR2実装をほぼそのまま移植。
// SigV4署名の実装自体はaws-sdk-s3に任せる（自前実装は署名バグのリスクが高いため避ける）。
// aws-config（環境変数等からの自動解決レイヤー）は使わず、単一の固定認証情報を
// Config::builderで直接組み立てる。

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct R2Config {
	#[serde(rename = "accountId", default)]
	pub account_id: String,
	#[serde(default)]
	pub bucket: String,
	#[serde(rename = "accessKeyId", default)]
	pub access_key_id: String,
	#[serde(rename = "secretAccessKey", default)]
	pub secret_access_key: String,
	#[serde(rename = "publicBaseUrl", default)]
	pub public_base_url: String,
}

#[derive(Debug, Serialize)]
pub struct R2Object {
	pub key: String,
	pub size: i64,
	#[serde(rename = "lastModified")]
	pub last_modified: String,
}

fn r2_config_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
	use tauri::Manager;
	let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
	Ok(dir.join("r2_config.json"))
}

fn r2_endpoint(account_id: &str) -> String {
	format!("https://{account_id}.r2.cloudflarestorage.com")
}

fn r2_client(config: &R2Config) -> Client {
	let creds = Credentials::new(
		&config.access_key_id,
		&config.secret_access_key,
		None,
		None,
		"shelf_gen",
	);
	let conf = aws_sdk_s3::Config::builder()
		.behavior_version(BehaviorVersion::latest())
		.region(Region::new("auto"))
		.endpoint_url(r2_endpoint(&config.account_id))
		.credentials_provider(creds)
		.force_path_style(true)
		.build();
	Client::from_conf(conf)
}

fn r2_public_url_for(config: &R2Config, key: &str) -> String {
	let base = if config.public_base_url.is_empty() {
		r2_endpoint(&config.account_id)
	}
	else {
		config.public_base_url.trim_end_matches('/').to_string()
	};
	format!("{base}/{key}")
}

#[tauri::command]
pub fn r2_load_config(app: tauri::AppHandle) -> Option<R2Config> {
	let path = r2_config_path(&app).ok()?;
	let data = std::fs::read_to_string(path).ok()?;
	serde_json::from_str(&data).ok()
}

#[tauri::command]
pub fn r2_save_config(app: tauri::AppHandle, config: R2Config) -> Result<(), String> {
	let path = r2_config_path(&app)?;
	if let Some(dir) = path.parent() {
		std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
	}
	let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
	std::fs::write(&path, json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn r2_public_url(config: R2Config, key: String) -> String {
	r2_public_url_for(&config, &key)
}

/// 指定キーが既に存在するか（内容アドレスのキーなら「アップロード済みか」の判定に使える。
/// 一致すればアップロード自体をスキップできる）
#[tauri::command]
pub async fn r2_object_exists(config: R2Config, key: String) -> Result<bool, String> {
	let client = r2_client(&config);
	let resp = client
		.list_objects_v2()
		.bucket(&config.bucket)
		.prefix(&key)
		.send()
		.await
		.map_err(|e| e.to_string())?;
	Ok(resp.contents().iter().any(|o| o.key() == Some(key.as_str())))
}

/// Shelfが作る配布物は `games/` または `manifest.json` 配下に限る前提で一覧する
/// （バケットを他用途と共用していても、一覧はこのツールが管理する範囲だけに絞る）
#[tauri::command]
pub async fn r2_list_objects(config: R2Config, prefix: Option<String>) -> Result<Vec<R2Object>, String> {
	let client = r2_client(&config);
	let resp = client
		.list_objects_v2()
		.bucket(&config.bucket)
		.prefix(prefix.unwrap_or_default())
		.send()
		.await
		.map_err(|e| e.to_string())?;
	Ok(resp
		.contents()
		.iter()
		// キーが"/"で終わる0バイトオブジェクトは実データではなくフォルダ表示用の
		// プレースホルダー。一覧に出すと誤削除の危険があるだけなので除外する
		.filter(|o| !o.key().unwrap_or_default().ends_with('/'))
		.map(|o| R2Object {
			key: o.key().unwrap_or_default().to_string(),
			size: o.size().unwrap_or_default(),
			last_modified: o.last_modified().map(|d| format!("{d:?}")).unwrap_or_default(),
		})
		.collect())
}

/// 指定prefix配下から keep_key 以外を削除する（旧版の自動掃除用）。
/// 削除件数を返す。アップロード成功「後」に呼ぶ前提（先に消すと失敗時にファイルが消失する）
#[tauri::command]
pub async fn r2_delete_others_with_prefix(config: R2Config, prefix: String, keep_key: String) -> Result<u32, String> {
	let client = r2_client(&config);
	let resp = client
		.list_objects_v2()
		.bucket(&config.bucket)
		.prefix(&prefix)
		.send()
		.await
		.map_err(|e| e.to_string())?;

	let mut deleted = 0u32;
	for obj in resp.contents() {
		let Some(key) = obj.key() else { continue };
		if key == keep_key { continue; }
		client
			.delete_object()
			.bucket(&config.bucket)
			.key(key)
			.send()
			.await
			.map_err(|e| format!("{key} の削除に失敗: {e}"))?;
		deleted += 1;
	}
	Ok(deleted)
}

/// 指定prefix配下から keep_keys に含まれないものを削除する（複数ファイルを一式で入れ替える公開物の旧版掃除用）。
/// 削除したキーを返す。アップロード成功「後」に呼ぶこと
#[tauri::command]
pub async fn r2_delete_not_in(config: R2Config, prefix: String, keep_keys: Vec<String>) -> Result<Vec<String>, String> {
	if prefix.is_empty() || !prefix.ends_with('/') {
		return Err("prefixは空でなく末尾が / であること".to_string());
	}
	let client = r2_client(&config);
	let resp = client
		.list_objects_v2()
		.bucket(&config.bucket)
		.prefix(&prefix)
		.send()
		.await
		.map_err(|e| e.to_string())?;
	let mut deleted: Vec<String> = Vec::new();
	for obj in resp.contents() {
		let Some(key) = obj.key() else { continue };
		if keep_keys.iter().any(|k| k == key) { continue; }
		client.delete_object().bucket(&config.bucket).key(key).send().await
			.map_err(|e| format!("{key} の削除に失敗: {e}"))?;
		deleted.push(key.to_string());
	}
	Ok(deleted)
}

/// オブジェクトをテキストとして読む。無ければNone（新旧比較用）
#[tauri::command]
pub async fn r2_get_text(config: R2Config, key: String) -> Result<Option<String>, String> {
	let client = r2_client(&config);
	match client.get_object().bucket(&config.bucket).key(&key).send().await {
		Ok(resp) => {
			let bytes = resp.body.collect().await.map_err(|e| e.to_string())?.into_bytes();
			Ok(Some(String::from_utf8_lossy(&bytes).to_string()))
		}
		Err(e) if e.as_service_error().is_some_and(|s| s.is_no_such_key()) => Ok(None),
		Err(e) => Err(e.to_string()),
	}
}

/// アップロード後の公開URL（表示用）を返す
#[tauri::command]
pub async fn r2_upload_file(config: R2Config, local_path: String, key: String) -> Result<String, String> {
	if key.is_empty() {
		return Err("保存先キー名を入力すること".to_string());
	}
	let client = r2_client(&config);
	let body = aws_sdk_s3::primitives::ByteStream::from_path(&local_path)
		.await
		.map_err(|e| format!("ファイル読み込みに失敗: {e}"))?;
	client
		.put_object()
		.bucket(&config.bucket)
		.key(&key)
		.body(body)
		.send()
		.await
		.map_err(|e| e.to_string())?;

	Ok(r2_public_url_for(&config, &key))
}

/// 既にメモリ上にあるバイト列（マニフェストなど）を直接アップロードする
/// （r2_upload_fileはローカルファイル前提なので、一時ファイルを作らずに済ませる別口）
#[tauri::command]
pub async fn r2_upload_bytes(config: R2Config, bytes: Vec<u8>, key: String) -> Result<String, String> {
	if key.is_empty() {
		return Err("保存先キー名を入力すること".to_string());
	}
	let client = r2_client(&config);
	let body = aws_sdk_s3::primitives::ByteStream::from(bytes);
	client
		.put_object()
		.bucket(&config.bucket)
		.key(&key)
		.body(body)
		.send()
		.await
		.map_err(|e| e.to_string())?;

	Ok(r2_public_url_for(&config, &key))
}

#[tauri::command]
pub async fn r2_delete_object(config: R2Config, key: String) -> Result<(), String> {
	let client = r2_client(&config);
	client
		.delete_object()
		.bucket(&config.bucket)
		.key(&key)
		.send()
		.await
		.map_err(|e| e.to_string())?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn r2_public_url_for_uses_public_base_url_when_set() {
		let config = R2Config {
			public_base_url: "https://pub-abc123.r2.dev/".to_string(),
			..Default::default()
		};
		assert_eq!(r2_public_url_for(&config, "games/foo/bar.dmg"), "https://pub-abc123.r2.dev/games/foo/bar.dmg");
	}

	#[test]
	fn r2_public_url_for_falls_back_to_account_endpoint_when_unset() {
		let config = R2Config { account_id: "myaccountid".to_string(), ..Default::default() };
		assert_eq!(
			r2_public_url_for(&config, "manifest.json"),
			"https://myaccountid.r2.cloudflarestorage.com/manifest.json",
		);
	}
}
