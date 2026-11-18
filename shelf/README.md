# Shelf

SKYNovel/BlueSNovel製ゲーム（同一サークルの複数作品）をまとめて管理するランチャー。
プレイヤーはマニュアルなしで使える想定。Lockstep（複数機器での同時動作テスト治具）とは
別アプリ。使い方・設計の経緯は `docs/manual.md` と `TODO.md`「Shelf」節を参照。

## 構成

```
shelf/
├── core/          UI非依存の判定ロジック（crate: shelf_core）
├── app/           ランチャー本体（プレイヤー向け・配布スタブ。crate: shelf）
├── gen/           生成ツール（開発者向け。crate: shelf_gen）
├── dev-fixtures/  生成ツールを使わずモック動作を確認するための材料
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

開発用フィクスチャは mac のパス・`.app` 構成が前提。Win で使うには、config の `manifestUrl` を
`file:///C:/...` 形式にし、`.app` ではなく `<名前>\resources\app.asar` の構成で偽インストールを作り、
`SHELF_WIN_DIRS` でその親フォルダを指す。Win の実インストール先は electron-builder がパッケージ名
（`package.json` の `name`）をフォルダ名に使うため、`appName`（表示名）と違うことがある。その場合は
config の `winInstallDirName` を設定する（生成ツールの「Winフォルダ名」欄）。

`ui/` を直したら `cargo build` をやり直す（`frontendDist` を埋め込む構成のため、JS だけ
直してアプリを再起動しても古いコードのまま）。

## ランチャーを作る手順

1. Win機で `bun run app:build` し、できた `shelf.exe`（`dist-local/`）を Mac の
   `gen/prebuilt/x86_64-pc-windows-msvc/` に置く（初回・ランチャー本体を変えたときだけ）
2. Mac で `bun run gen:dev` を起動
3. サークル・作品・R2・出力先を入力して「ランチャーを生成」
4. 生成後、マニフェストを R2 に公開
5. できた dmg と exe を配る

生成ツールはこのリポジトリが手元にあるMacでしか動かない（生成のたびに `app/` のmacスタブを
ビルドし直す）ので、dmg 化して配布するものではない。`gen:build` は不要。
`app:build` はスタブ単体の動作確認用で、成果物は `dist-local/`（gitignore済み）に出る。
Winインストーラー（NSIS exe）から `setting.sn` を抽出して購入者チェックに使うには、生成するMacに
7-Zip（`brew install sevenzip`）が要る。無いと本体チェックサムによる代替になり、生成ツールの作品カードに警告が出る。

## 生成ツール（開発者向け）

```sh
npm run gen:dev
```

使い方は `docs/manual.md`「Shelfに自分のゲームを登録する」参照。署名鍵は
`~/.shelf-gen/signing-key.json`（リポジトリ外）、Win向けstubは
`gen/prebuilt/x86_64-pc-windows-msvc/shelf.exe`（.gitignore済み、Windows実機でビルドして
blues-sync経由で配置）。
