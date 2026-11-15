//! Shelf のUI非依存の判定ロジック一式。
//!
//! 購入者チェック・体験版判定・配布物の検証・インストールは、UIフレームワーク
//! （Tauri）に依存せずここに置く。`app/src-tauri` がこれを呼ぶだけの薄いコマンド層になる。
//! 旧実装（famibee/bluesnovel-vsce の sn_legacy_patch。コミット41c9c1b）からの移植を
//! 多く含む。設計の経緯はそのコミットの `src/docs/legacy-app-patch.md` 参照。

pub mod asar;
pub mod checksum;
pub mod config;
pub mod detect;
pub mod dialog;
pub mod download;
pub mod footer;
pub mod inspect;
pub mod install;
pub mod launch;
pub mod manifest;
pub mod status;
pub mod version;
