// SKYNovel/BlueSNovel製アプリの自動更新（[update_check]タグ）用ファイルの公開支援。
// 仕様: https://famibee.github.io/bluesnovel/tag.html#update_check
//   - urlは更新ファイルを置くWebフォルダ（末尾"/"必須）
//   - build/package/update の中身（_index.json＋各OS/arch用インストーラー）をそのまま置く
//   - インストーラー名は毎ビルドで変わる（暗号化名）ため、公開時は旧ファイルを掃除する

use serde::Serialize;
use std::path::{Path, PathBuf};

const UPDATE_DIR: &str = "build/package/update";
const MAIN_SN: &str = "doc/prj/script/main.sn";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchFile {
	pub name: String,
	pub path: String,
	pub size: u64,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PatchInspect {
	/// 更新フォルダが存在し_index.jsonがあるか
	pub available: bool,
	pub files: Vec<PatchFile>,
	/// _index.jsonのversion
	pub version: String,
	/// main.snの[update_check]のurl（無ければ空）
	pub current_url: String,
	pub main_sn_exists: bool,
	pub warnings: Vec<String>,
}

fn main_sn_path(project: &Path) -> PathBuf {
	project.join(MAIN_SN)
}

/// 行頭（空白除く）が[update_check で始まり、コメント行でない行を探し、url='...'の値を返す
fn find_update_check_url(text: &str) -> Option<String> {
	for line in text.lines() {
		let t = line.trim_start();
		if !t.starts_with("[update_check") { continue; }
		let rest = &t["[update_check".len()..];
		let at = rest.find("url=")?;
		let v = &rest[at + 4..];
		let (quote, body) = match v.chars().next()? {
			q @ ('\'' | '"') => (Some(q), &v[1..]),
			_ => (None, v),
		};
		let end = match quote {
			Some(q) => body.find(q)?,
			None => body.find(|c: char| c.is_whitespace() || c == ']').unwrap_or(body.len()),
		};
		return Some(body[..end].to_string());
	}
	None
}

/// [update_check]行のurl値だけを差し替える。他の属性（cond等）・行内の他の文字は保つ。
/// 変更が無ければNone、[update_check]行が無ければErr
fn replace_update_check_url(text: &str, new_url: &str) -> Result<Option<String>, String> {
	let mut done = false;
	let mut changed = false;
	let mut out = String::with_capacity(text.len() + 64);
	for seg in text.split_inclusive('\n') {
		let t = seg.trim_start();
		if !done && t.starts_with("[update_check") {
			if let Some(at) = seg.find("url=") {
				let vstart = at + 4;
				let v = &seg[vstart..];
				let (qlen, quote) = match v.chars().next() {
					Some(q @ ('\'' | '"')) => (1, Some(q)),
					_ => (0, None),
				};
				let body = &v[qlen..];
				let end = match quote {
					Some(q) => body.find(q),
					None => body.find(|c: char| c.is_whitespace() || c == ']'),
				};
				if let Some(end) = end {
					let old = &body[..end];
					done = true;
					if old != new_url {
						changed = true;
						out.push_str(&seg[..vstart + qlen]);
						out.push_str(new_url);
						out.push_str(&seg[vstart + qlen + end..]);
						continue;
					}
				}
			}
		}
		out.push_str(seg);
	}
	if !done { return Err("main.snに url= を持つ [update_check] 行が見つからない".to_string()); }
	Ok(if changed { Some(out) } else { None })
}

#[tauri::command]
pub fn patch_inspect(project_folder: String) -> PatchInspect {
	let project = Path::new(&project_folder);
	let mut r = PatchInspect::default();
	let dir = project.join(UPDATE_DIR);
	let index = dir.join("_index.json");
	if !index.is_file() { return r; }

	match std::fs::read_to_string(&index).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
		Some(v) => r.version = v.get("version").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
		None => r.warnings.push("_index.json を解釈できない".to_string()),
	}
	if let Ok(rd) = std::fs::read_dir(&dir) {
		for e in rd.flatten() {
			let name = e.file_name().to_string_lossy().to_string();
			if name.starts_with('.') { continue; }
			let Ok(md) = e.metadata() else { continue };
			if !md.is_file() { continue; }
			r.files.push(PatchFile { name, path: e.path().to_string_lossy().to_string(), size: md.len() });
		}
	}
	// _index.jsonは最後にアップロードする（先に新しい索引を出すと、まだ無いファイルを指すため）
	r.files.sort_by(|a, b| (a.name == "_index.json").cmp(&(b.name == "_index.json")).then(a.name.cmp(&b.name)));
	r.available = true;

	let ms = main_sn_path(project);
	if let Ok(text) = std::fs::read_to_string(&ms) {
		r.main_sn_exists = true;
		r.current_url = find_update_check_url(&text).unwrap_or_default();
	}
	r
}

/// main.snの[update_check]のurlを書き換える。書き換えたらtrue、既に同じならfalse
#[tauri::command]
pub fn patch_set_update_check_url(project_folder: String, url: String) -> Result<bool, String> {
	if !url.ends_with('/') { return Err("urlは末尾が / であること".to_string()); }
	let ms = main_sn_path(Path::new(&project_folder));
	let text = std::fs::read_to_string(&ms).map_err(|e| format!("{} を読めない: {e}", ms.display()))?;
	match replace_update_check_url(&text, &url)? {
		Some(new_text) => {
			std::fs::write(&ms, new_text).map_err(|e| format!("{} に書けない: {e}", ms.display()))?;
			Ok(true)
		}
		None => Ok(false),
	}
}

// --- 新旧チェック ---
// 元（doc/prj）→ 暗号化出力（doc_crypto/prj）→ バンドル（build/package のdmg/exe）→ update一式 → R2
// の鎖で、上流が下流より新しい（＝変更が下流に入っていない）ものを警告する。
// 更新時刻ベースの判定なので「内容が変わったか」までは見ない（誤検出もありうる）。
// 版・ファイルサイズ・R2の索引との食い違いは確定的な不一致として error にする。

use std::time::SystemTime;

/// 手順（鎖）の1工程。levelは notes のうち最も重いもの（無ければ ok）
#[derive(Debug, Serialize)]
pub struct Step {
	/// source / crypto / mainsn / mac / win / index / r2
	pub id: &'static str,
	pub title: &'static str,
	/// その工程の成果物の更新時刻（表示用。無ければ空）
	pub time: String,
	/// "error"（確定的な不一致）/ "warn"（更新時刻からの推定）/ "ok" / "skip"（確認できない・対象外）
	pub level: &'static str,
	pub notes: Vec<String>,
}

struct Steps(Vec<Step>);

impl Steps {
	fn new() -> Self {
		Steps([
			("source", "元（doc/prj）"), ("crypto", "暗号化出力（doc_crypto/prj）"), ("mainsn", "main.sn の update_check"),
			("mac", "mac バンドル（dmg）"), ("win", "win バンドル（exe）"),
			("index", "update 一式（_index.json）"), ("r2", "R2 公開"),
		].map(|(id, title)| Step { id, title, time: String::new(), level: "ok", notes: Vec::new() }).into())
	}
	fn get(&mut self, id: &str) -> &mut Step {
		self.0.iter_mut().find(|s| s.id == id).expect("known step id")
	}
	fn note(&mut self, id: &str, level: &'static str, msg: impl Into<String>) {
		let st = self.get(id);
		st.notes.push(msg.into());
		let rank = |l: &str| match l { "error" => 3, "warn" => 2, "skip" => 1, _ => 0 };
		if rank(level) > rank(st.level) { st.level = level; }
	}
}

fn newest_mtime(dir: &Path) -> Option<SystemTime> {
	let mut newest: Option<SystemTime> = None;
	let mut stack = vec![dir.to_path_buf()];
	while let Some(d) = stack.pop() {
		let Ok(rd) = std::fs::read_dir(&d) else { continue };
		for e in rd.flatten() {
			let name = e.file_name().to_string_lossy().to_string();
			if name.starts_with('.') || name == "path.json" { continue; }
			let Ok(md) = e.metadata() else { continue };
			if md.is_dir() { stack.push(e.path()); continue; }
			if let Ok(t) = md.modified() {
				if newest.map_or(true, |n| t > n) { newest = Some(t); }
			}
		}
	}
	newest
}

fn mtime(p: &Path) -> Option<SystemTime> {
	std::fs::metadata(p).ok()?.modified().ok()
}

fn fmt_time(t: SystemTime) -> String {
	use std::process::Command;
	let secs = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
	// 外部依存を増やさないため、表示用の整形だけdateコマンドに任せる（mac/Linux。失敗時は秒数）
	Command::new("date").args(["-r", &secs.to_string(), "+%Y-%m-%d %H:%M"]).output().ok()
		.and_then(|o| String::from_utf8(o.stdout).ok())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.unwrap_or_else(|| format!("epoch {secs}"))
}

/// _index.jsonのOS別エントリ（path・sizeを持つオブジェクト）を (キー, エントリ) で列挙
fn index_entries(v: &serde_json::Value) -> Vec<(String, &serde_json::Value)> {
	v.as_object().map(|o| {
		o.iter().filter(|(_, e)| e.get("path").is_some() && e.get("cn").is_some())
			.map(|(k, e)| (k.clone(), e)).collect()
	}).unwrap_or_default()
}

/// ローカルとR2の_index.jsonの食い違い（版・OS別のsha512/cn）を文章で返す。同じなら空
fn compare_index(local: &serde_json::Value, remote: &serde_json::Value) -> Vec<String> {
	let mut diffs = Vec::new();
	let ver = |v: &serde_json::Value| v.get("version").and_then(|x| x.as_str()).unwrap_or("?").to_string();
	if ver(local) != ver(remote) {
		diffs.push(format!("版が違う（R2: v{} / ローカル: v{}）", ver(remote), ver(local)));
	}
	for (k, e) in index_entries(local) {
		let r = remote.get(&k);
		match r {
			None => diffs.push(format!("{k} がR2の索引に無い")),
			Some(r) => {
				if r.get("sha512") != e.get("sha512") || r.get("cn") != e.get("cn") {
					diffs.push(format!("{k} の内容がR2と違う"));
				}
			}
		}
	}
	for (k, _) in index_entries(remote) {
		if local.get(&k).is_none() { diffs.push(format!("{k} はR2にだけある")); }
	}
	diffs
}

/// OS別エントリのキー（darwin_x64等）→ 工程id
fn step_of_key(key: &str) -> &'static str {
	if key.starts_with("darwin") { "mac" } else if key.starts_with("win") { "win" } else { "index" }
}

/// remote_checked=false: R2を確認していない（設定なし等）。true で remote_index=None: R2に索引が無い
#[tauri::command]
pub fn patch_check(project_folder: String, remote_checked: bool, remote_index: Option<String>) -> Vec<Step> {
	let project = Path::new(&project_folder);
	let mut st = Steps::new();
	let dir = project.join(UPDATE_DIR);

	let source = newest_mtime(&project.join("doc/prj"));
	let crypto_dir = project.join("doc_crypto/prj");
	let has_crypto = crypto_dir.is_dir();
	let crypto = if has_crypto { newest_mtime(&crypto_dir) } else { None };
	let main_sn = mtime(&main_sn_path(project));
	st.get("source").time = source.map(fmt_time).unwrap_or_default();
	st.get("crypto").time = crypto.map(fmt_time).unwrap_or_default();
	st.get("mainsn").time = main_sn.map(fmt_time).unwrap_or_default();
	if !has_crypto { st.note("crypto", "skip", "この作品は暗号化していない（doc_crypto/prj なし）"); }
	if let (Some(s), Some(c)) = (source, crypto) {
		if s > c { st.note("crypto", "warn", format!("元（{}）より古い。暗号化し直しが必要", fmt_time(s))); }
	}

	let Ok(local_text) = std::fs::read_to_string(dir.join("_index.json")) else {
		st.note("index", "error", "build/package/update/_index.json が無い。配布形式を生成すること");
		return st.0;
	};
	let Ok(local) = serde_json::from_str::<serde_json::Value>(&local_text) else {
		st.note("index", "error", "_index.json を解釈できない");
		return st.0;
	};
	let local_ver = local.get("version").and_then(|x| x.as_str()).unwrap_or_default().to_string();
	st.get("index").time = mtime(&dir.join("_index.json")).map(fmt_time).unwrap_or_default();

	// 版: package.jsonとupdate一式
	let pkg_ver = std::fs::read_to_string(project.join("package.json")).ok()
		.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
		.and_then(|v| v.get("version").and_then(|x| x.as_str()).map(String::from));
	if let Some(v) = pkg_ver {
		if v != local_ver {
			st.note("index", "error", format!("package.json の版（v{v}）と update 一式の版（v{local_ver}）が違う。バンドルが古い"));
		}
	}

	// バンドルの材料: 暗号化する作品は暗号化出力、しない作品は元
	let (material, material_name) = if has_crypto { (crypto, "暗号化出力") } else { (source, "元") };

	let mut bundle_times: Vec<(&'static str, SystemTime)> = Vec::new();
	for (key, e) in index_entries(&local) {
		let id = step_of_key(&key);
		let path = e.get("path").and_then(|x| x.as_str()).unwrap_or_default();
		let cn = e.get("cn").and_then(|x| x.as_str()).unwrap_or_default();
		let size = e.get("size").and_then(|x| x.as_u64());
		let upd = dir.join(format!("{key}-{cn}"));
		let pkg = project.join("build/package").join(format!("v{local_ver}")).join(path);
		for (label, f) in [("update一式", &upd), ("パッケージ", &pkg)] {
			match std::fs::metadata(f) {
				Err(_) => st.note(id, "error", format!("{label}のファイルが無い（{}）", f.display())),
				Ok(md) if size.is_some_and(|s| s != md.len()) => st.note(id, "error", format!(
					"{label}のサイズが _index.json と違う（{} / 索引 {}）", md.len(), size.unwrap_or(0))),
				_ => {}
			}
		}
		if let Some(t) = mtime(&upd) {
			if id != "index" {
				st.get(id).time = fmt_time(t);
				bundle_times.push((id, t));
			}
			if let Some(m) = main_sn {
				if m > t { st.note(id, "warn", format!("main.sn（{}）より古い。変更が入っていない", fmt_time(m))); }
			}
			if let Some(m) = material {
				if m > t { st.note(id, "warn", format!("{material_name}（{}）より古い。再バンドルが必要な可能性", fmt_time(m))); }
			}
		}
	}
	// 片方のOSしか配布しない作品: _index.json に無いOSは対象外（skip）。
	// ただし build/package に当該OSのパッケージが有るのに update 一式に入っていない場合は、入れ忘れの可能性があるので警告する
	for (id, ext) in [("mac", ".dmg"), ("win", ".exe")] {
		if !(st.get(id).time.is_empty() && st.get(id).notes.is_empty()) { continue; }
		let has_pkg = std::fs::read_dir(project.join("build/package").join(format!("v{local_ver}"))).ok()
			.is_some_and(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().ends_with(ext)));
		if has_pkg {
			st.note(id, "warn", format!("build/package に {ext} は有るが update 一式（_index.json）に入っていない。配布するなら配布形式を生成し直す"));
		}
		else {
			st.note(id, "skip", "この作品に当該OS版は無い（update 一式にも含まれない）。配布しないなら対応不要");
		}
	}
	// OS間でバンドル時刻が大きく離れている＝片方だけ作り直した可能性
	if let (Some(min), Some(max)) = (
		bundle_times.iter().min_by_key(|(_, t)| *t), bundle_times.iter().max_by_key(|(_, t)| *t),
	) {
		let gap = max.1.duration_since(min.1).map(|d| d.as_secs()).unwrap_or(0);
		if gap > 1800 {
			st.note(min.0, "warn", format!("もう一方のOSより{}分古い。片方だけ作り直していないか", gap / 60));
		}
	}

	// R2との新旧
	if !remote_checked {
		st.note("r2", "skip", "R2を確認できていない（ホスティング設定、または main.sn に公開URLが未設定）");
	}
	else {
		match remote_index.as_deref().map(serde_json::from_str::<serde_json::Value>) {
			None => st.note("r2", "warn", "R2に _index.json が無い（未公開）"),
			Some(Err(_)) => st.note("r2", "warn", "R2の _index.json を解釈できない"),
			Some(Ok(remote)) => {
				let diffs = compare_index(&local, &remote);
				if diffs.is_empty() { st.note("r2", "ok", "手元の update 一式と一致（公開済み）"); }
				else { for d in diffs { st.note("r2", "warn", d); } }
			}
		}
	}
	st.0
}

#[cfg(test)]
mod tests {
	use super::*;

	const SRC: &str = "\t[event key=p]\n\t[update_check url='https://x.r2.dev/patch/a/b/' cond=!const.体験版]\n[macro name=lr]\n";

	#[test]
	fn finds_url() {
		assert_eq!(find_update_check_url(SRC).as_deref(), Some("https://x.r2.dev/patch/a/b/"));
		assert_eq!(find_update_check_url("[jump fn=a]\n"), None);
	}

	#[test]
	fn replaces_only_url_keeping_cond() {
		let out = replace_update_check_url(SRC, "https://y/z/").unwrap().unwrap();
		assert!(out.contains("[update_check url='https://y/z/' cond=!const.体験版]"));
		assert!(out.starts_with("\t[event key=p]\n"));
	}

	#[test]
	fn compare_index_detects_version_and_hash() {
		let a: serde_json::Value = serde_json::from_str(r#"{"version":"1.2.0","darwin_x64":{"path":"a","cn":"c","sha512":"x"}}"#).unwrap();
		let b: serde_json::Value = serde_json::from_str(r#"{"version":"1.1.0","darwin_x64":{"path":"a","cn":"c","sha512":"y"}}"#).unwrap();
		assert!(compare_index(&a, &a).is_empty());
		let d = compare_index(&a, &b);
		assert_eq!(d.len(), 2);
		assert!(d[0].contains("版が違う") && d[1].contains("darwin_x64"));
	}

	#[test]
	fn newest_mtime_ignores_hidden_and_path_json() {
		let dir = std::env::temp_dir().join(format!("shelf_patch_test_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		assert!(newest_mtime(&dir).is_none());
		std::fs::write(dir.join(".DS_Store"), "x").unwrap();
		std::fs::write(dir.join("path.json"), "x").unwrap();
		assert!(newest_mtime(&dir).is_none());
		std::fs::write(dir.join("a.sn"), "x").unwrap();
		assert!(newest_mtime(&dir).is_some());
		let _ = std::fs::remove_dir_all(&dir);
	}

	/// win版だけの作品の一時プロジェクトを作る（extra_dmg=trueなら build/package に dmg だけ置く）
	fn win_only_project(tag: &str, extra_dmg: bool) -> std::path::PathBuf {
		let root = std::env::temp_dir().join(format!("shelf_winonly_{tag}_{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&root);
		let upd = root.join("build/package/update");
		let pkg = root.join("build/package/v1.0.0");
		std::fs::create_dir_all(&upd).unwrap();
		std::fs::create_dir_all(&pkg).unwrap();
		std::fs::write(root.join("package.json"), r#"{"version":"1.0.0"}"#).unwrap();
		std::fs::write(upd.join("_index.json"), r#"{"version":"1.0.0","name":"x","win32_x64":{"path":"x-1.0.0-x64.exe","size":3,"sha512":"s","cn":"CN"}}"#).unwrap();
		std::fs::write(upd.join("win32_x64-CN"), "abc").unwrap();
		std::fs::write(pkg.join("x-1.0.0-x64.exe"), "abc").unwrap();
		if extra_dmg { std::fs::write(pkg.join("x-1.0.0-x64.dmg"), "abc").unwrap(); }
		root
	}

	fn level_of(steps: &[Step], id: &str) -> &'static str {
		steps.iter().find(|s| s.id == id).unwrap().level
	}

	#[test]
	fn single_os_project_marks_missing_os_as_skip() {
		let root = win_only_project("a", false);
		let steps = patch_check(root.to_string_lossy().to_string(), false, None);
		assert_eq!(level_of(&steps, "mac"), "skip");
		assert_ne!(level_of(&steps, "win"), "error");
		assert_eq!(level_of(&steps, "index"), "ok");
		let _ = std::fs::remove_dir_all(&root);
	}

	#[test]
	fn missing_os_with_package_present_warns() {
		let root = win_only_project("b", true);
		let steps = patch_check(root.to_string_lossy().to_string(), false, None);
		assert_eq!(level_of(&steps, "mac"), "warn");
		let _ = std::fs::remove_dir_all(&root);
	}

	#[test]
	fn same_url_is_noop_and_missing_is_err() {
		assert_eq!(replace_update_check_url(SRC, "https://x.r2.dev/patch/a/b/").unwrap(), None);
		assert!(replace_update_check_url("[jump fn=a]\n", "https://y/").is_err());
	}
}
