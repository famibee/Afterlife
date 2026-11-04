# Afterlife — 開発時の留意事項

経緯や未完了事項は `TODO.md` を参照。ここには、何度も踏みそうな作業上の知見だけを置く。

## 起動時のロール切り替え（AI操作用）

`--role=host` / `--role=client` 引数、または `AFTERLIFE_ROLE` 環境変数で起動すると、
`RoleSelect` 画面を飛ばして親機/子機の画面へ直接入る（`src-tauri/src/lib.rs` の
`initial_role` コマンド、`src/routes/+page.svelte` の `onMount`）。HostPad/ClientSetup は
マウント時に自動で `start_host` / 発見処理を呼ぶので、UIクリックなしでそこまで進む。
mac/Win双方のAIエージェントが手動クリックせずに実機検証を進める際はこれを使う。

```
AFTERLIFE_ROLE=host npm run tauri dev
AFTERLIFE_ROLE=client npm run tauri dev
```

## 親機ネットワーク受信の2方式

- 既定: `bun`サイドカー経由（`src-tauri/sidecar/host-server.ts`）。未署名Rustネイティブ
  バイナリの着信TCP listenがAvast等に遮断される問題への対処。
- `AFTERLIFE_DIRECT_NET=1` 環境変数で、Bunを介さずRust本体が直接listenする版
  （`net/server.rs` の `start_direct`）に切り替えられる。Avast側の設定変更で
  直接listenが通るか切り分けたいときに使う。

## macOSのdev検証で踏みやすい点

- Bashの `timeout` コマンドはmacには無い（coreutils未導入環境）。バックグラウンド起動
  （`(cmd &)`）+ `sleep` + `pkill -f` の組で代用する。
- `npm run tauri dev` は初回/Rust側再コンパイル時に数十秒かかる。ログに
  `Running `target/debug/afterlife`` 等が出るまで `sleep`→`tail` で待つ。
- macOSのTCC権限（Accessibility等）はコード署名の同一性に紐づく。`cargo tauri dev` は
  再ビルドごとにバイナリ（署名）が変わるため権限が無効化・再プロンプトされうる。
  権限が絡む検証は、安定した署名を持つビルド済み `.app` に対して行う
  （リリースビルドで画面が真っ白になる既知バグがあるため、現状は検証手段自体が制約される。
  `TODO.md` 参照）。

## 相手機（Win/mac）が実際に接続中は、devモードで追加のビルド・編集をしない

`npm run tauri dev`はソース変更を検知するたびにRustプロセスを強制終了して再起動し、
親機ならbunサイドカーも道連れで再起動する。相手機が接続した状態でこれを短時間に
何度も行うと、古いbunサイドカーの終了が新しいbunの起動に間に合わず
「Error: Failed to start server. Is port 47821 in use?」で新しいbunが即座に終了し、
親機プロセス自体は生きているのにポートがlistenされていない（＝相手から見るとTCPが
`SYN_SENT`のまま応答なし）状態に陥ることを2026-11-05に実際に再現した（`TODO.md`参照）。
相手機と協調して実機テストをしている最中は、`cargo build`/`touch`などサーバーを
不要に再起動させる操作を避ける。コード修正が必要な場合は、相手機に一旦待ってもらって
から行い、全プロセスを`pkill`してクリーンに再起動してから再開する。

## `static/` 配下のオーバーレイ用HTML

`WebviewUrl::App("foo.html")` で参照するファイルは `static/foo.html` に置く。
`npm run build` で `build/` に反映されるが、devモード中にRust側だけ素早く確認したい場合は
`cp static/foo.html build/foo.html` でも一時的に揃えられる（本来は `npm run build` が正本）。

## Windows: 2枚目のWebView2を同期生成するとデッドロックする

メインスレッド上で2つ目のWebView2ウィンドウ（ハイライト枠オーバーレイ、クリック位置の
視覚フィードバックなど）を同期生成すると、WebView2自身の非同期初期化がそのスレッドの
メッセージポンプを要求するためデッドロックする（`run_on_main_thread` 越しでも再現する）。
この種の別ウィンドウ機能を追加する際は、`#[cfg(windows)]` で早期returnして無効化し、
ネイティブな非WebView実装に置き換えるまで待つ（`client_show_highlight`/
`client_hide_highlight`/`show_click_feedback` と同じパターン）。
