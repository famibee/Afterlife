# Shelf

SKYNovel/BlueSNovel製ゲーム（同一サークルの複数作品）をまとめて管理するランチャー。
プレイヤーはマニュアルなしで使える想定。Lockstep（複数機器での同時動作テスト治具）とは
別アプリ。使い方・設計の経緯は `docs/manual.md` と `TODO.md`「Shelf」節を参照。

## 構成

```
shelf/
├── core/          UI非依存の判定ロジック（crate: shelf_core）
├── app/           ランチャー本体（プレイヤー向け・配布スタブ。crate: shelf）
├── gen/           生成ツール（開発者向け。第2段階で実装）
├── dev-fixtures/  生成ツールが無い段階でモック動作を確認するための材料
└── docs/          マニュアル
```

## 開発

```sh
npm install
# core のテスト（rustupのRust版を明示する。Homebrew版がPATHで勝つため）
PATH="$HOME/.cargo/bin:$PATH" RUSTC="$HOME/.cargo/bin/rustc" RUSTDOC="$HOME/.cargo/bin/rustdoc" cargo test -p shelf_core

# 開発用フィクスチャの生成（設定・マニフェスト・署名・偽インストール一式を dev-fixtures/ に作る）
PATH="$HOME/.cargo/bin:$PATH" RUSTC="$HOME/.cargo/bin/rustc" cargo run -p shelf_core --bin gen_dev_fixtures

# モックで起動（実機の/Applicationsを汚さない）
SHELF_CONFIG=$PWD/dev-fixtures/config.json \
SHELF_MAC_APPS_DIR=$PWD/dev-fixtures/fake-install \
PATH="$HOME/.cargo/bin:$PATH" RUSTC="$HOME/.cargo/bin/rustc" \
npm run app:dev
```

`ui/` を直したら `cargo build` をやり直す（`frontendDist` を埋め込む構成のため、JS だけ
直してアプリを再起動しても古いコードのまま）。
