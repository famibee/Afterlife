// 検出済みインストールを起動する。成功後にランチャーを終了するかどうかは呼び出し側
// （app/src-tauri側のコマンド）が決める（core側はプロセス起動のみに責務を絞る）。

use crate::detect::{self, Platform};
use std::io;
use std::path::Path;
use std::process::Command;

// SKYNovel/BlueSNovel製ゲームはElectron製。`ELECTRON_RUN_AS_NODE` が環境変数にセットされた
// ままだと、起動したElectronアプリがNodeとして動いてしまい、ウィンドウを出さず数秒で終了する
// （2026-11-16・Win実機で確認。Claude Codeの開発シェルがこの変数を継承していたことが原因。
// 通常のエンドユーザー環境では付かない変数だが、起動した子プロセスへ無条件に継承させる理由も
// 無いので、Shelf自身がどんな環境から起動されても安全なように明示的に除去しておく）
fn clear_electron_run_as_node(command: &mut Command) -> &mut Command {
	command.env_remove("ELECTRON_RUN_AS_NODE")
}

pub fn launch_installed_app(install_path: &Path, platform: Platform, app_name: &str) -> io::Result<()> {
	match platform {
		Platform::Mac => {
			clear_electron_run_as_node(Command::new("/usr/bin/open").arg(install_path)).spawn()?;
			Ok(())
		}
		Platform::Windows => {
			let exe = detect::installed_binary_path(install_path, platform, app_name)
				.ok_or_else(|| io::Error::other("実行ファイルが見つからない"))?;
			clear_electron_run_as_node(&mut Command::new(exe)).spawn()?;
			Ok(())
		}
		Platform::Other => Err(io::Error::other("対象外プラットフォーム")),
	}
}
