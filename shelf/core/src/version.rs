// 版文字列の比較。semver相当だが厳密なsemver実装は要らない（配布側がelectron-builderの
// 慣例的な "x.y.z" を出す前提。パースできない成分は0として扱い、落ちない方を優先する）。

fn parse_tuple(s: &str) -> (u64, u64, u64) {
	let mut parts = s.trim().splitn(3, '.');
	let mut next = || parts.next().and_then(|p| p.parse::<u64>().ok()).unwrap_or(0);
	(next(), next(), next())
}

/// remoteがinstalledより新しいか（厳密に大きいかのみ。等しい・古い・パース不能はfalse）
pub fn is_newer(remote: &str, installed: &str) -> bool {
	parse_tuple(remote) > parse_tuple(installed)
}

pub fn is_same(remote: &str, installed: &str) -> bool {
	parse_tuple(remote) == parse_tuple(installed)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn is_newer_compares_numeric_segments() {
		assert!(is_newer("1.3.0", "1.2.9"));
		assert!(is_newer("2.0.0", "1.9.9"));
		assert!(!is_newer("1.2.0", "1.2.0"));
		assert!(!is_newer("1.2.0", "1.3.0"));
	}

	#[test]
	fn is_newer_handles_missing_patch_segment() {
		assert!(is_newer("1.3", "1.2.9"));
	}

	#[test]
	fn unparsable_segments_fall_back_to_zero() {
		// "beta" 等の非数値成分は0扱い。壊れたバージョン文字列で誤って「更新あり」と
		// 誤判定しないよう、安全側（falseになりやすい）に倒れることを確認する
		assert!(!is_newer("1.x.0", "1.0.0"));
	}

	#[test]
	fn is_same_requires_exact_numeric_match() {
		assert!(is_same("1.2.3", "1.2.3"));
		assert!(!is_same("1.2.3", "1.2.4"));
	}
}
