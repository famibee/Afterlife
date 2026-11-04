# TODO / 引き継ぎメモ

現時点で未完了・保留中の事項のみ。経緯や完了済みの作業は各ファイルの実装・コミットメッセージを参照（このファイルには書かない）。

## 動作未確認・要検証

- **右クリック・ドラッグ・キー入力の配信**: クリック(左)と座標変換精度はmac↔Win実機間で確認済みだが、右クリック・ドラッグ・キー入力はまだ試していない。
- **macOS側でのクリック精度**: mac自身をホストにしたmac→mac自己テストでは、クリック着弾・視覚フィードバック表示を確認済み（2026-11-04）。Win側を親機にした「Win→mac」方向の実機確認は依頼中・回答待ち。
- **mDNS発見がIPv4ではなくIPv6リンクローカルのみを返すことがある**: 2026-11-04の検証で、mac側ホストの`enable_addr_auto()`がIPv4アドレスを含まず、Win側の発見結果が`fe80::...%11`のIPv6リンクローカルのみだった（原因未調査）。クライアント側の`preferredAddress`はIPv4を優先するが、IPv4自体が無ければIPv6にフォールバックする。クライアント側URL構築の「invalid authority」クラッシュ（ゾーンID付きIPv6を`[]`で囲まずURLに入れていた）は`net/client.rs`の`format_ws_host`で修正済み（ゾーンIDは破棄するので、リンクローカルIPv6自体はルーティングできずクラッシュしないだけ）。恒久対応は親機側で確実にIPv4を広告すること。

## 未解決バグ

- **リリースビルドで画面が真っ白になる**: `cargo build --release`で作ったバイナリ（または署名目的で直接起動したバイナリ）を実行すると、ウィンドウは開くがコンテンツが真っ白になる。`npm run tauri dev`（Viteの開発サーバー経由）では問題なく表示される。原因未調査（SvelteKit static adapterの出力とTauriの埋め込みアセットプロトコルの組み合わせ、またはプロダクションモード特有のJSエラーの可能性）。これが直らないと、署名済みの安定したバイナリで動作確認する手段がなく、開発モードのホットリロードのたびに再ビルドされる不安定な状態が続く。

## 未実装・設計検討

- **クリック位置の視覚フィードバック**: mac側は実装・動作確認済み（`src-tauri/src/lib.rs`の`show_click_feedback`、`static/click-feedback.html`。クリック着弾位置に別ウィンドウで●を表示しCSSアニメーションでフェードアウト）。2026-11-04、mac自身をホストにした自己テスト(mac→mac)でTextEditドキュメント中央への着弾と●の表示をスクリーンショットで確認済み。検証の過程で、`platform/macos.rs`の`parse_bounds`が`kCGWindowBounds`の値を直接のキー(X/Y/Width/Height)として辞書のトップレベルから探していたバグ(実際は`kCGWindowBounds`キーの値自体がX/Y/Width/Heightを持つ入れ子の辞書)を発見・修正した。このバグにより、macが子機になる場合は`list_windows`/`window_rect`が常に失敗しており、クリック注入自体が機能しない状態だった（Win側検証では一度も踏んでいなかった）。Windowsは既知のWebView2デッドロック問題（`client_show_highlight`と同じ理由）により無効化したままなので、ネイティブな非WebView実装への置き換えが別途必要。
  - 設計調査(2026-11-04、ユーザー依頼によるweb検索): MIT公開されているMicrosoft PowerToysの「Mouse Highlighter」「Find My Mouse」(`MouseHighlighter.cpp`/`FindMyMouse.cpp`)が同種の機能を実装済み。専用のWin32ウィンドウクラスを登録した透明・レイヤードなオーバーレイウィンドウを作り、グローバルマウスフックで座標を取得して最前面に描画している。WebViewは使わず素のWin32ウィンドウ+GDI/Composition描画。定石は`WS_EX_LAYERED | WS_EX_TRANSPARENT`でクリックスルー、`WS_EX_NOACTIVATE`+`SetWindowPos`の`SWP_NOACTIVATE`でフォーカスを奪わない。この構成ならTauriの2枚目WebView2デッドロックを根本的に回避できる。Windows側実装時はこの方針（既存依存の`windows`クレートで`CreateWindowExW`/`SetLayeredWindowAttributes`等）を採用する。Barrier/Synergyは参考にならない（疑似カーソルではなく実カーソルを移動する方式）。
- **Bunランタイムの同梱**: 現状、親機のネットワーク受信は`bun run sidecar/host-server.ts`を開発機にインストール済みの`bun`実行ファイルで起動している。配布時はBunランタイム本体をTauriのリソースとして同梱し、`tauri.conf.json`の`resources`経由で絶対パスを解決する必要がある（`net/server.rs`の`resolve_sidecar_script`は既にdev/packaged両対応のつもりで書いてあるが、Bun本体の同梱は未着手）。
- **デバッグ用 `/debug/send` エンドポイント**: `sidecar/host-server.ts`に、127.0.0.1限定でクリック/キー配信を直接トリガーできるデバッグ用HTTPエンドポイントを追加した（自動テスト用）。配布前に無効化または削除を検討する。
- **第2段階（グローバルフック）**: 計画どおり未着手。現状は親機ウィンドウ内の操作パッドのみ。macOS/WindowsそれぞれのCGEventTap/SetWindowsHookExによる実装方針は、設計時の計画ファイル（`NOTES.private.md` 参照）に記載済み。

## 既知の制限（優先度低）

- **macOSの`window_rect`がタイトルバーを含む**: `platform/macos.rs`の`list_windows`/`window_rect`は`CGWindowListCopyWindowInfo`の`kCGWindowBounds`（ウィンドウ全体の枠）を使っており、Win側の`GetClientRect`（クライアント領域のみ）と違い、タイトルバー分だけ上端にズレがある。今回の精度検証はWin側のみだったため、mac側のこのズレは未計測。
- **WindowsのDPI対応**: Per-Monitor-V2 DPI awarenessのmanifest宣言は未確認。高DPI環境・複数モニタでの座標ズレは未検証。
