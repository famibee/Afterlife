// 自己参照データの読み取り・埋め込み（legacy-app-patch.md「実装基盤（論点2）」の設計を踏襲）。
//
// Windows配布物は [汎用スタブ本体][JSON][u32 LE: JSONバイト長][8バイトのマジック] という形を
// している。生成ツール側がこのバイナリの末尾にプロジェクト固有データ（config::EmbeddedConfig
// をJSON化したもの）を単純に連結するだけで1ファイルの配布物を作れる。
//
// このファイル自体はバイト列の読み書きだけを扱い、JSONの中身（スキーマ）には関与しない
// （スキーマは config.rs 側が持つ）。旧実装（sn_legacy_patch）は `Config = Vec<AppConfig>` を
// 直接知っていたが、Shelfでは footer と config を分離し、footer は汎用の「末尾連結フォーマット」
// のみの責務にした。
//
// macOS 向けはこの末尾連結方式を使わない（.app/Contents/Resources/shelf-config.json に置く。
// 2026-11-16 実機検証：Resources配下への追加だけでもad-hoc署名の封印は壊れる
// 〈`codesign --verify --deep --strict` が `a sealed resource is missing or invalid` になる〉。
// 生成ツールが config 埋め込み後に `codesign -s - --force --deep` で再署名することが前提。
// 実行中のバイナリ末尾への追記はこの封印の話とは無関係だが、Win側の単一exe配布という
// 要件に合わせてWindows限定の方式として残している）。

pub const MAGIC: &[u8; 8] = b"SHELFCFG";

/// stub（汎用バイナリ本体）の末尾に JSON を連結する。順序：json ++ len(u32 LE) ++ magic
pub fn append_footer(stub: &[u8], json: &str) -> Vec<u8> {
	let json_bytes = json.as_bytes();
	let mut out = Vec::with_capacity(stub.len() + json_bytes.len() + 4 + MAGIC.len());
	out.extend_from_slice(stub);
	out.extend_from_slice(json_bytes);
	out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
	out.extend_from_slice(MAGIC);
	out
}

/// full_bytes は実行ファイル全体のバイト列（実運用では自分自身のファイルを丸ごと読む）。
/// マジックが無ければ None（連結前の裸のスタブを直接実行した場合、または macOS など
/// この方式を使わないプラットフォームの場合）
pub fn extract_trailing_json(full_bytes: &[u8]) -> Option<String> {
	let tail_len = MAGIC.len() + 4;
	if full_bytes.len() < tail_len { return None; }

	let (rest, magic) = full_bytes.split_at(full_bytes.len() - MAGIC.len());
	if magic != MAGIC { return None; }

	let (rest2, len_bytes) = rest.split_at(rest.len() - 4);
	let json_len = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;
	if rest2.len() < json_len { return None; }	// 壊れたデータ（長さがおかしい）

	let json_bytes = &rest2[rest2.len() - json_len..];
	String::from_utf8(json_bytes.to_vec()).ok()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn append_then_extract_roundtrips() {
		let stub = b"fake-binary-bytes";
		let json = r#"{"schema":1,"apps":[]}"#;
		let full = append_footer(stub, json);

		assert_eq!(extract_trailing_json(&full).as_deref(), Some(json));
	}

	#[test]
	fn extract_returns_none_without_magic() {
		assert_eq!(extract_trailing_json(b"just a stub with no footer"), None);
	}

	#[test]
	fn extract_returns_none_for_too_short_input() {
		assert_eq!(extract_trailing_json(b"short"), None);
	}

	#[test]
	fn extract_returns_none_when_length_is_corrupted() {
		let stub = b"stub";
		let json = r#"{"a":1}"#;
		let mut full = append_footer(stub, json);
		// 長さフィールド（マジックの直前4バイト）を壊す
		let len_pos = full.len() - MAGIC.len() - 4;
		full[len_pos..len_pos + 4].copy_from_slice(&9999u32.to_le_bytes());

		assert_eq!(extract_trailing_json(&full), None);
	}
}
