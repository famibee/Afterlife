# TODO / 引き継ぎメモ

現時点で未完了・保留中の事項のみ。経緯や完了済みの作業は各ファイルの実装・コミットメッセージを参照（このファイルには書かない）。

## 動作未確認・要検証

- **右クリック・ドラッグ・キー入力の配信**: クリック(左)と座標変換精度はmac↔Win実機間で確認済みだが、右クリック・ドラッグ・キー入力はまだ試していない。
- **macOS側でのクリック精度**: 今回の実機検証はWin側（子機）でのみ実施。mac側が子機になった場合の精度・フォーカス復帰は未検証。
- **Win側のmDNS残骸**: `stop_host`で`ServiceDaemon::shutdown()`を呼ぶよう修正済み（直らない原因はdropのみで goodbye パケットが送られないことだった）。Win側が修正版を同期・再ビルドした後、実際に残骸（旧インスタンス名が一覧に残り続ける現象）が解消するか未確認（2026-11-04、Win側セッションへ確認依頼済み・回答待ち）。
- **bunサイドカーなし直接TCP listen版**: Avastの「webガード」をOFFにした状態で、未署名Rustバイナリが直接listenしてもブロックされないか試す（ユーザー2026-11-04発見）。`AFTERLIFE_DIRECT_NET=1`環境変数で`server::start_direct`（Bunサイドカーを介さず`net/server.rs`内で直接`TcpListener`+`tokio-tungstenite`でlisten）に切り替えられる実装済み。実機での動作確認は未実施。通れば後述の「Bunランタイムの同梱」作業は不要になる。

## 未解決バグ

- **リリースビルドで画面が真っ白になる**: `cargo build --release`で作ったバイナリ（または署名目的で直接起動したバイナリ）を実行すると、ウィンドウは開くがコンテンツが真っ白になる。`npm run tauri dev`（Viteの開発サーバー経由）では問題なく表示される。原因未調査（SvelteKit static adapterの出力とTauriの埋め込みアセットプロトコルの組み合わせ、またはプロダクションモード特有のJSエラーの可能性）。これが直らないと、署名済みの安定したバイナリで動作確認する手段がなく、開発モードのホットリロードのたびに再ビルドされる不安定な状態が続く。

## 未実装・設計検討

- **クリック位置の視覚フィードバック**: mac側は実装済み（`src-tauri/src/lib.rs`の`show_click_feedback`、`static/click-feedback.html`。クリック着弾位置に別ウィンドウで●を表示しCSSアニメーションでフェードアウト）。dev起動して落ちないことのみ確認、実機でのクリック注入を伴う目視確認は未実施。Windowsは既知のWebView2デッドロック問題（`client_show_highlight`と同じ理由）により無効化したままなので、ネイティブな非WebView実装への置き換えが別途必要。
- **Bunランタイムの同梱**: 現状、親機のネットワーク受信は`bun run sidecar/host-server.ts`を開発機にインストール済みの`bun`実行ファイルで起動している。配布時はBunランタイム本体をTauriのリソースとして同梱し、`tauri.conf.json`の`resources`経由で絶対パスを解決する必要がある（`net/server.rs`の`resolve_sidecar_script`は既にdev/packaged両対応のつもりで書いてあるが、Bun本体の同梱は未着手）。
- **デバッグ用 `/debug/send` エンドポイント**: `sidecar/host-server.ts`に、127.0.0.1限定でクリック/キー配信を直接トリガーできるデバッグ用HTTPエンドポイントを追加した（自動テスト用）。配布前に無効化または削除を検討する。
- **第2段階（グローバルフック）**: 計画どおり未着手。現状は親機ウィンドウ内の操作パッドのみ。macOS/WindowsそれぞれのCGEventTap/SetWindowsHookExによる実装方針は、設計時の計画ファイル（`NOTES.private.md` 参照）に記載済み。

## 既知の制限（優先度低）

- **macOSの`window_rect`がタイトルバーを含む**: `platform/macos.rs`の`list_windows`/`window_rect`は`CGWindowListCopyWindowInfo`の`kCGWindowBounds`（ウィンドウ全体の枠）を使っており、Win側の`GetClientRect`（クライアント領域のみ）と違い、タイトルバー分だけ上端にズレがある。今回の精度検証はWin側のみだったため、mac側のこのズレは未計測。
- **WindowsのDPI対応**: Per-Monitor-V2 DPI awarenessのmanifest宣言は未確認。高DPI環境・複数モニタでの座標ズレは未検証。
