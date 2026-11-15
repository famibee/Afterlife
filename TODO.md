# TODO / 引き継ぎメモ

現時点で未完了・保留中の事項のみ。経緯や完了済みの作業は各ファイルの実装・コミットメッセージを参照（このファイルには書かない）。

このリポジトリには複数のアプリがある。**節ごとにどの話かを分けてある。**

| アプリ | 場所 | 状態 |
|---|---|---|
| **Lockstep**（新） | `lockstep/` | 開発中。使い方は `lockstep/README.md` |
| **Shelf**（新） | `shelf/` | 開発中。第1段階（モックランチャー）・第2段階（生成ツール）実装済み。使い方は `shelf/README.md` |
| **Afterlife**（旧） | repoルート（`src/`・`src-tauri/`） | 2026-11-10 に開発を止めた。**凍結のまま保持する**（2026-11-11 決定。削除しない） |

Lockstepはノベルゲームを複数機器で一手ずつ同時に進め画面を目視比較する自分用の治具、Shelfは
SKYNovel/BlueSNovel製ゲーム（同一サークルの複数作品）をまとめて管理するランチャー。
互いに独立したアプリ。旧Afterlifeは汎用リモート操作ツール化して完成が見えなくなったため、
最小構成のLockstepに作り直した（Shelfとは無関係）。

---

# Lockstep（新アプリ・`lockstep/`）

## 検証待ち

mac 単体（Intel iMac Pro、親機・子機を同一機で起動、対象は TextEdit）では 2026-11-10・11 に次を確認済み: リリースビルドで画面が出る／クリックを一度も送らずに最初のキーが入る／別アプリ（VS Code）前面からの自動前面化で誤入力なく対象へ入る／対象ウィンドウが閉じられても同じアプリ名で探し直して入る／「数え直す」操作で全機の集計が0に戻る／クリック着弾（計算値 (630.5, 368.5) に対し (631, 369)、セッション層イベントタップで計測・2回）／親機ローカル注入とフォーカス戻し／子機の自動再接続（子機先行・親機再起動の両方）。

Win 実機は 2026-11-10 に単体検証済み。

**機器間（LAN 越し）の疎通・同時操作は、両方向とも 2026-11-11 に確認済み。**

- **Win 親機 → mac 子機**: キー(h,i,←,x,5)→クリックを送信し、mac 側の TextEdit に "hx5i" が正しく入った。mac 側の「成功/失敗」表示も Win 側の親機表示と完全に一致。
- **mac 親機 → Win 子機**: 同様にキー・クリックとも成功。クリック着弾位置を、パッド内5点（中央・四隅近く）× Win 側の計測用小窓（878x644px、DPI 150%）で測定し、**誤差は全点とも1px以内**（パッド位置の実測誤差 ±1px の範囲内）。座標変換・注入は両方向とも正確に機能している。
  - 検証中に踏んだ罠（実装の問題ではない）: ①座標指定クリックに使った検証用ツール（CGEvent合成）で `clickState` フィールドを明示しておらず、OS が連続クリックと誤認識して1クリックが2手としてカウントされた。②親機自身が対象を選んでいる場合、クリックごとに「対象へ前面化→注入→Lockstepへ前面復帰」のサイクルが走り、復帰前に次の操作を送ると届かない（2.5秒間隔では失敗、4秒間隔で確実に成功）。後者は「既知の制限」に追記済み。

**mac のアプリケーションファイアウォールの初期設定について**: mac を親機にした直後は Win 子機からの着信が TCP 確立直後に切断される症状が出た（127.0.0.1 からの自己接続は成功）。`sudo socketfilterfw --add`/`--unblockapp` での許可登録・親機の再起動を行っても解消せず、最終的に**ファイアウォールを一旦まるごと無効化してから再度有効化・許可し直す**ことで解消した（許可リストへの登録自体は正しく反映されていたが、実行中バイナリへの反映に通常の再起動では不十分だった可能性がある。原因は未特定）。配布物を署名するまでは、初回セットアップ時にこの手順が要ることとして記録しておく。

**実際のゲーム（大阪九龍条）は Win 単体（親機・子機を同一機、127.0.0.1）で 2026-11-11 に検証済み。** タイトル→章選択→第1章開始→本文クリックで1行ずつ進行、Enter で本文送り、連続送信（Enter を2秒間隔で6回）でも「成功6手」で失敗・取りこぼしなし。前面化・フリーズ・意図しない操作、いずれも問題なし。クリック注入は低レベル計測（マウスフック）でも、目標座標に対し3px以内・移動からカーソル復帰まで約17msで完了していることを確認。

**Win 実機で Avast CyberCapture による PC フリーズが発生（2026-11-11）。** リリース版（署名なし exe）を新しくビルドし直した直後に Win 機自体がフリーズし、ユーザーが PC を再起動する事態になった。影響で Win 子機・ゲームとも終了し、mac 親機との接続も切れた。

- **訂正**: 同日に一度「Avast の反応は確認した範囲では無し」と報告していたが、これは誤り。署名なしの新しい exe を作るたびに CyberCapture の対象になりうると考えること。
- 旧 Afterlife でも同種の報告があり（開発ビルドは再ビルドのたびにハッシュが変わる未署名バイナリのため、Avast が毎回ヒューリスティック解析する）、Lockstep でも実際に再現することが確認できた。
- 対応案（Avast の除外設定に `lockstep` の `target/` フォルダを加える）はあるが、ユーザー自身の判断が要るセキュリティ設定なので、Win 側セッションからは変更していない。**未対応。**
- 配布時の案内文で注意喚起する運用（「参考知見」節参照）は変わらないが、**開発中の頻繁な再ビルド自体もリスクである**ことが新しく分かった。

- **mac 子機での実ゲーム操作**: 未検証（mac に実ゲームを導入していない）。
- **Windows の DPI 100% 以外・複数モニタでのクリック着弾**: DPI 150%・単一モニタでは誤差1px以内を複数回確認済み（Win 側、2026-11-10・11）。100%・複数モニタは未検証。

**Apple Silicon 機（M6）は 2026-11-11 に検証完了。** universal 版 dmg（`lockstep/Lockstep_0.1.0_universal.dmg`、x86_64+arm64、ad-hoc 署名）をインストールし、アクセシビリティ許可後、mac(Intel)親機↔M6子機でキー・クリックの同期を確認。mac・Win・M6 の3台構成でも「成功」件数が一致した。

- **踏んだ罠（配布物の作り方の問題）**: 最初に配っていた universal dmg は **11/10 にビルドした古いバージョン**で、Status/Reset/Ping プロトコル追加（11/11 の修正）を含んでいなかった。M6 からのクリック・キー適用自体（`ServerMsg::Click`/`Key`）は旧プロトコルでも成立するため正常に動くが、M6→親機への結果報告（`ClientMsg::Status`、旧版では`Result`という別形式）は親機側が新しい`ClientMsg` enumにデシリアライズできず`Err(_) => {}`で黙って握りつぶされ、親機画面は「受信待ち」のまま変化しなかった。クリック・キー自体は効くのに報告だけ届かないという紛らわしい症状になるので、**プロトコルを変更したら配布済みdmgは必ず作り直す**（`npm run tauri build -- --target universal-apple-darwin`）。dmg内バイナリの実際のビルド日時は `hdiutil attach` でマウントし `stat -f "%Sm" <バイナリ>` で確認できる。
- **新規接続した子機の「取りこぼし」表示が過大になる**: 子機が接続する前に親機が送っていた分の連番との差を「取りこぼし」として数えてしまう（例: 親機が8手まで進めた後に新しい子機が繋ぐと、最初の1手で「取りこぼし8手」と出る）。実害はない（接続前のそもそも受け取りようがない操作なので）が紛らわしい表示。子機接続時に親機側の現在のseqを通知し、`expected_next`をそれで初期化すれば直る見込み。未対応、優先度低。
- **mac の入力ソースが日本語（かな）だと、注入したキーが変換される**: M6 の TextEdit で "a" が "あ" に、Enter が改行でなく変換確定になった。Lockstep側の注入（物理キーコード送出）は正しく動いているが、IME が有効だと想定通りの文字にならない。英数入力に切り替えれば解消する。検証用に文字入力で確認するときは、対象機の入力ソースを先に確認すること。

## 未実装・検討

- **グローバルホットキー**（例: F9 で一手進む）。ユーザーは「設定の手間がなければ興味あり」。固定キーなら設定不要。親機でゲームを前面にしたまま操作でき、フォーカスの往復が要らなくなる。`tauri-plugin-global-shortcut` が1つ増える。
- **Bluetooth など LAN 以外の通信**: 採らない（複数台のペアリングが LAN より手間）。当初要件にはあったので記録だけ残す。
- **長押し**（Ctrl スキップ等）: 捨てると決めた（2026-11-10）。キーはタップのみ。

## 既知の制限

- **mac は入力をセッション層へ送る**（`inject_macos.rs`）。Deskflow が HID 層に張るタップを避けるため。HID を直接読むゲーム（IOKit・GameController 経由）には届かない可能性がある。Electron 製なら問題ない見込み。
- **mac の前面化はアプリ単位**。同じアプリの複数ウィンドウは撃ち分けられない。
- **クリック基準が OS で違う**: mac はタイトルバー込みの枠、Win はクライアント領域。位置が重要なときはフルスクリーンで動かす。
- **Windows の DPI・複数モニタ**: 未検証（上の Win 実機検証に含めた）。
- **役割の切り替えは再起動**。停止・切断の処理を持たないことで単純にしている。
- **認証なし**。同じ LAN の誰でも親機に繋げ、子機は繋いだ先の操作をそのまま入れる。自宅 LAN 前提。
- **親機自身も対象を選んでいる（ローカル注入あり）と、操作の間隔を詰めすぎると取りこぼす**。1操作ごとに「対象へ前面化→注入→Lockstepへ前面復帰」のサイクルが走り、復帰の完了前に次の操作が来ると、そのクリック/キーは前面にある対象側（別ウィンドウ扱いのOS側）に吸われて Lockstep に届かない。2026-11-11 のクロスマシン検証（合成クリックで複数点を連続送信）で、2.5秒間隔では毎回失敗し、4秒間隔なら確実に成功することを実測した。手動でのクリック・キー操作では気にする間隔ではないが、自動連打・スクリプトからの連続送信では間を空ける。

## 開発時の注意

- **Rust は rustup 版を使う**。PATH では Homebrew 版が先に来る。universal ビルドと Windows 向け型検査は `PATH="$HOME/.cargo/bin:$PATH"` を付ける。Windows 向け型検査は `llvm-rc`（`/usr/local/Cellar/llvm/<版>/bin`）も PATH に要る。
- **`CARGO_TARGET_DIR` に相対パスを渡さない**。Tauri は `src-tauri/` で cargo を動かすので `src-tauri/src-tauri/target/` ができる（2026-11-10 に踏んだ。gitignore の対象外になる）。
- **`ui/` の変更は自動反映されない**。`devUrl` を使わず静的配信しているため、アプリを再起動して確かめる。
- **mac で入力注入が効かないときは、まず Deskflow を疑う**。マウスが別の画面にある間、HID 層へ送った合成入力は黙って捨てられる（権限は正常・エラーなし）。`CGGetEventTapList` で `deskflow-core` のタップを確認できる。Lockstep 自体はセッション層送出で回避済みだが、検証用の別ツール（enigo、`CGEventPost(kCGHIDEventTap)`）は影響を受ける。
- **`ui/` を編集したら `cargo build` を必ずやり直す**。`devUrl` を使わず `frontendDist` を直接埋め込む構成なので、JS だけ直してアプリを再起動しても古いコードのまま（2026-11-11 に踏んだ）。
- 自動検証は `--autosend`（デバッグビルドのみ）で親機から操作を流し、対象アプリの中身やイベントタップで結果を測る。子機を親機より先に起動し、起動直後の `list_windows` に対象がまだ出ないことがある（タイミング起因、一過性。`--target` の自動選択に失敗したら「更新」ボタンで取り直せる）。

---

# 旧アプリ Afterlife（repoルート・開発停止）

以下は旧アプリの未完了事項。**Lockstep では下表のとおり、作りごと無くなったか別の形で解決した。** 旧アプリを再開しない限り対応は不要。

| 旧アプリの項目 | Lockstep では |
|---|---|
| 画面比をスナップショットから自動取得 | スナップショット転送を持たない。親機が選んだ対象ウィンドウの縦横比を使う |
| キー入力が間欠的に効かない | クリックとキーの注入経路を1本にし、前面化の完了を確かめてから入れる。下の「2026-11-10 に分かったこと」参照 |
| mDNS が IPv6 リンクローカルのみ返す | mDNS を持たない（アドレス手入力） |
| Vite フルリロードでロール状態がずれる | Vite・SvelteKit を使わない（静的 HTML+JS） |
| Win 実機が無反応になる／孤児プロセスのポート競合 | Bun サイドカーを持たない。Win 実機での再発有無は検証待ち |
| リリースビルドで画面が真っ白 | 起きない（mac のリリースビルドで表示を確認済み） |
| Bun ランタイムの同梱／`/debug/send` | Bun サイドカーを持たない |
| 第2段階（グローバルフック） | 親機が自分のゲームへローカル注入するので不要 |
| クリック位置の視覚フィードバック | 持たない |
| mac の `window_rect` がタイトルバーを含む／Windows の DPI | 同じ制限が残る（Lockstep の「既知の制限」） |

**2026-11-10 に分かったこと（旧アプリのキー入力間欠不良に関係）**
- `lib.rs` のキー処理は `Injector` を初期化しない（初期化はクリック経路の `perform_click` だけ）。そのセッションで一度もクリックを通していない子機ではキーが入らない。これが「送信して直後に効くときと効かないときがある」の主因と見られる。
- クリック後に `restore_focus` で前面を戻し、次のキーで `activate_window` した直後、前面化の完了を待たずに注入している点も一因になりうる。
- mac 側では Deskflow の影響もありうる（Lockstep「開発時の注意」参照）。enigo は mac で HID 層固定。Win 側への影響は未確認。

---

# Shelf（新アプリ・`shelf/`）

SKYNovel/BlueSNovel製ゲーム（同一サークルの複数作品）をまとめて管理するランチャー。
設計の経緯・旧実装（`famibee/bluesnovel-vsce` のパッチ生成ツール）からの移植元は下の
「参考知見」節参照。進め方は ①モックランチャー → ②生成ツール → ③モックを実装で置き換え。
**第1段階（モックランチャー）は実装済み。**

## 検証待ち

- **自動インストール手順の実機確認が途中。** M6（Apple Silicon・macOS 27.2.1）で
  「ad-hoc署名の.app内Resourcesへの設定ファイル追加」「dmgから/Applicationsへのditto置き換え」を
  2026-11-16に検証したが、両方とも**ターミナル/VS Code拡張という既に権限が通っている起動元**からの
  実行だった。**Shelf.app自身（未許可の起動元）から同じditto操作をしたときに「アプリ管理」権限に
  止められないかは未確認。** Shelf実装が進んだ段階で改めて確認する。
- **Resourcesへの設定追加はad-hoc署名の封印を壊す（2026-11-16実機確認済み）。**
  `codesign --verify --deep --strict` が `a sealed resource is missing or invalid` になる。
  生成ツール（`shelf/gen/src-tauri/src/stub_build.rs`の`stamp_and_resign_mac_app`）が
  config埋め込み後に`codesign -s - --force --deep`で再署名することで対処済み。
  **mac universalビルド→設定埋め込み→再署名→dmg化→`codesign --verify --deep --strict`の
  通しを実機テストで確認済み**（`cargo test -p shelf_gen -- --ignored`、2026-11-16）。
  生成されたdmgを実際に起動してShelf本体が動くかまでは未確認（次の検証項目）。
  - 踏んだ罠：`shelf/app/src-tauri`はworkspaceメンバーのため、ビルド成果物は
    `shelf/app/src-tauri/target/`ではなくworkspace共有の`shelf/target/`に出る。さらに
    `cargo tauri build --target universal-apple-darwin`は`bundle/macos/`配下に.appを
    残さず、.appはビルド直後の`bundle/dmg/*.dmg`の中にしか存在しない。`rebuild_mac_universal_app()`は
    このdmgを一旦マウントして.appを取り出す実装にして解決した。
- **Windowsのサイレント上書きインストール（`/S`）は per-user インストールでのみ実機確認済み**
  （2026-11-16。UAC無し・Avast無反応で成功）。**全ユーザー向け（perMachine）インストールでの
  UAC挙動は未検証。**
- **生成ツール（`shelf/gen/`）のR2アップロード・マニフェスト公開は未検証。**
  mac dmg生成（universalビルド→設定埋め込み→再署名→dmg化）は実機テストで確認済み
  （上記参照）。スキャン→チェックサム抽出→R2アップロード→マニフェスト公開は
  ユニットテストのみ（`project_scan.rs`・`extract.rs`・`r2.rs`・`signing_key.rs`）で、
  実プロジェクト・実R2資格情報・GUI操作込みの通し検証はまだ（ネイティブのファイル選択
  ダイアログがこのセッションでは補助アクセス権限の都合で自動操作できなかったため）。
- **Windows向けインストーラーからの`setting.sn`抽出（`extract_app_asar_from_nsis_exe`）は
  この開発機に7-Zipが無く未検証。** `which 7z`/`7za`/`7zz`で検出する実装のみ済み。
  mac向け（dmg・hdiutil経由）は実装済みだが実インストーラーでの検証はまだ。
- **Shelf子機（Win/mac）での実ゲーム操作・実際のインストール上書きは未検証。** ① は
  `dev-fixtures/` の偽インストールのみで検証済み（`SHELF_MAC_APPS_DIR`・`SHELF_CONFIG` で実機の
  `/Applications` を汚さずに検証できる）。

## 未実装・検討

- **段階1（Cloudflare Workerによる所持証明付き短命URL）**: マニフェストの `grantUrl` フィールドの
  口だけ用意済み（`shelf/core/src/manifest.rs`）。Worker本体・nonce設計・リプレイ対策は未設計。
  Cloudflareをホスティングに使う場合のみ必要（段階0のままでも動く）。
- **GitHub Actionsでのスタブビルド**: 生成ツールは現状ローカルビルド
  （`cargo tauri build --target universal-apple-darwin`を生成のたびに実行）。
  このリポジトリに`.github/`は一度も存在したことがない。`gh`のトークンに`workflow`権限が
  無いため、ワークフローファイルの初回pushは`gh auth refresh -s workflow`が必要になる可能性がある。
- **`crypto:true`プロジェクトの`settingSnFileName`自動計算**: 旧ツール（TS側）は
  `uuidv5(relPath, pass)`で算出するが、正確なアルゴリズムが未検証のため実装を見送った
  （2026-11-16決定。誤実装は「全購入者のチェックサムが永久に不一致になる」事故になるため）。
  現状は生成ツールの画面で手入力させる。
- **`pick_legacy_installer` コマンドの実機確認**: osascriptのファイル選択ダイアログ自体は
  旧実装からの無改修移植（`core/src/dialog.rs`）。`verify_legacy_installer`（チェックサム照合）は
  ユニットテスト済みだが、ダイアログ操作込みの手動確認は、このセッションにosascriptの補助アクセス
  権限が無く行えなかった（2026-11-16）。
- **ドラッグ＆ドロップ**: 生成ツールのUIは旧ツールと違いネイティブダイアログのみ
  （ファイル選択ボタン）。D&amp;Dは見送った（2026-11-16決定。後回しで追加可能）。

## 既知の制限

- 購入者チェック（`setting.sn`照合）が使えない過去出荷ビルドがある。詳細・フォールバックは
  `shelf/docs/manual.md`「購入者チェックが使えない場合」参照。
- Lockstepと同じ制限（macは入力注入を持たないため非該当だが）：macの前面化はアプリ単位、
  クリック基準のOS差は非該当（Shelfはクリック注入を行わない）。
- **署名鍵（ed25519）は`~/.shelf-gen/signing-key.json`に平文保存**（2026-11-16決定。リポジトリ外・
  0600権限。紛失すると既配布済みランチャー全てが新マニフェストを検証できなくなる。生成ツールの
  「サークル情報」タブにこの警告を表示している）。R2の資格情報（`r2_config.json`）も
  旧ツールと同じく平文保存（アプリ設定フォルダ内）。

## 開発時の注意

- **起動するゲームはElectron製なので、`ELECTRON_RUN_AS_NODE`環境変数が残っているとNodeとして
  動きウィンドウを出さず数秒で終了する**（2026-11-16・Win実機で確認。原因はClaude Codeの開発
  シェルがこの変数を継承していたこと）。通常のエンドユーザー環境では付かないはずの変数だが、
  `launch.rs`・`download.rs`で起動する子プロセスから明示的に`env_remove`で除去済み。**Shelf自体を
  Claude Codeのシェル経由で手動検証するときも、このシェル自身がこの変数を持っている前提で
  疑うこと**（mac側の`CLAUDE.md`「Electronアプリの動作確認」と同じ注意点）。
- `shelf/` は `core/`・`app/`・`gen/`を1つのCargo workspaceにまとめている
  （Lockstepは単独クレートだが、Shelfは`core`をapp/genで共有するため構成が違う）。
  クレート名は3つとも別にする（`shelf_core`・`shelf`・`shelf_gen`。同名にすると
  `path`依存が解決できない既知の罠、Lockstep開発時にも踏んだもの）。
- 生成ツール（`shelf/gen/`）は`npm run gen:dev`/`gen:build`で起動する。R2アップロード先は
  `games/<appSlug>/<os>/<sha256先頭16桁>/<ファイル名>`、マニフェストは固定キー
  `manifest.json`・`manifest.json.sig`（内容アドレスにしない。埋め込み設定の`manifestUrl`が
  指す先はビルド後に変わってはいけないため）。
- **生成したランチャー配布物（Win exe・mac dmg）自体のR2アップロードは既定では行わない**
  （開発者が自分のサイトで配る1点もの、という位置づけ）。「作品管理」タブの
  「生成した配布物もR2へアップロードする」をオンにすると、`games/`（ゲーム更新用の
  インストーラー）とは別名前空間の`launcher/<os>/<sha256先頭16桁>/<ファイル名>`に
  置ける（2026-11-16ユーザー指摘：パッチ配布物のすぐ隣に置くのを避けたいとの要望に対応）。
- Win向けstubは`shelf/gen/prebuilt/x86_64-pc-windows-msvc/shelf.exe`に置く運用
  （Lockstep・旧patch_gen_guiと同じ、Windows実機でビルドしblues-sync経由で配置。
  `.gitignore`済みなので開発機に無くても`gen/`自体は動く。無ければ生成時にwin側だけ
  スキップする設計）。
- Rustはrustup版を使う（Homebrew版がPATHで勝つ）。`PATH="$HOME/.cargo/bin:$PATH"
  RUSTC="$HOME/.cargo/bin/rustc"` を付ける。
- `SHELF_CONFIG`・`SHELF_MAC_APPS_DIR`・`SHELF_WIN_DIRS` 環境変数で、生成ツールが無い段階でも
  実機の`/Applications`等を汚さずに動作確認できる（`shelf/dev-fixtures/`・`gen_dev_fixtures`参照）。
- `ui/`を編集したら`cargo build`をやり直す（Lockstepと同じ、`frontendDist`埋め込み構成のため）。

---

# 参考知見: パッチ生成ツール（bluesnovel-vsce）

拡張機能 BlueSNovel のリポジトリ https://github.com/famibee/bluesnovel-vsce にある、同じ系統（Tauri v2 + Rust、dmg/exe 単体配布の小アプリ）の開発記録。**Lockstep に機能を足すときは、このリポジトリの過去ソースと設計メモを先に見る。** ローカルのソース（`~/Documents/MacHD2/_Famibee/SKYNovel/sn_extension`）は削除予定なので、参照は GitHub 側で行う。

| 見るもの | 場所（リポジトリ内） |
|---|---|
| 設計メモ（判断の根拠・実測・踏んだ罠。約900行） | `src/docs/legacy-app-patch.md` |
| 開発者向け GUI（Tauri v2 + 素の HTML/JS、R2 アップロード付き） | `patch_gen_gui/` |
| プレイヤー向け配布アプリ（Rust lib + Tauri ランチャー UI） | `patch_app/` |
| 生成ロジック（TS） | `src/genLegacyPatch.ts`・`LegacyAppCheck.ts`・`InstallerExtract.ts`・`DownloadUrlCheck.ts` |

パッチ生成ツール自体の残作業（ランチャー UI 化の実装順序7以降など）は、あちらの `src/docs/TODO.md` と `legacy-app-patch.md` が正本。ここには写さない。

## Lockstep にそのまま効く知見

**ビルド環境**
- `cargo`/`rustc` は Homebrew 版と rustup 版が二重にあり、PATH では Homebrew 版が勝つ。`~/.cargo/bin/cargo` を指定するだけでは足りず、cargo が内部で呼ぶ `rustc` も PATH 解決なので、`RUSTC=~/.cargo/bin/rustc`（doctest があれば `RUSTDOC` も）を明示する。
- mac の universal 版は `cargo tauri build --target universal-apple-darwin` が x64/arm64 の結合まで行う（手動 `lipo` は不要。両ターゲットの `rustup target add` が前提）。
- **Windows 版は Windows 実機でビルドする**。mac からのクロスコンパイル環境（mingw-w64・cargo-xwin）は不要と判明済みだが、それは「Win 実機に rustup があればそのままビルドできる」という意味。成果物の受け渡し（Win でビルド → blues-sync → mac）は手動のまま。
- 親クレートと `src-tauri/` のクレートで `[package] name` を同じにすると、`path = ".."` 依存を解決できず失敗する。
- `.gitignore` に `src-tauri/target/` と `src-tauri/gen/schemas/` を入れ忘れると巨大な生成物が追跡対象に出る。

**Tauri の挙動**
- `frontendDist` はビルド時にバイナリへ埋め込まれる。実行ファイル末尾へのデータ追記（`[本体][JSON][u32 LE 長][マジック]`）をしても表示は崩れず、起動時に自分自身から読み戻せる（2026-10-22 実機確認）。機器ごとの設定を1ファイルに持たせたいときに使える。
- `devUrl` を設定せず `frontendDist` を静的配信する構成では、ファイル監視の対象は `src-tauri/` の Rust だけ。`ui/` の変更はアプリを再起動しないと見えない。
- Rust を編集すると `tauri dev` が自動で再ビルド・再起動する。ログの `Finished` → `Running` の後に行が増えなくなるのを確かめてから次の操作をする（同じ `target/` の取り合いも避ける）。
- Tauri は Electron ではない。OS の WebView（Win: WebView2、mac: WKWebView）を使うだけで約 25MB。WebView2 は Win10 1803 以降は標準搭載。
- egui（eframe）は約 4.5MB と小さいが、カード UI・日本語フォント・スレッド処理を手組みする工数が大きく、Tauri へ乗り換えた（2026-10-22 決定。再検討不要）。`bun build --compile` は約 82MB で却下。GUI なしの素の Rust なら約 331KB。

**GUI アプリを AI から起動・確認するときの罠**（あちらの `.claude/skills/patch-gen-gui-dev` より。git 管理外なのでここへ写した）
- Bash ツールの `run_in_background: true` を使い、コマンドには `&` を付けない。両方やると二重にバックグラウンド化され、気づかずに同じアプリが2つ起動する。起動のたびに `ps` で PID の数を確かめる。
- `ELECTRON_RUN_AS_NODE` を外して起動する（`env -u ELECTRON_RUN_AS_NODE`）。
- 画面確認は、対象 PID を前面に出してから `screencapture -x`。「起動した」で済ませず画像を開いて見る。
- クリックは `osascript -e 'tell application "System Events" to click at {X, Y}'`。受け取るのは論理座標で、`screencapture` の物理ピクセル（Retina は2倍）とも、画像ビューアの縮小表示とも違う。返り値に当たった要素名が出るので、狙いどおりか確かめる。
- 確認が終わったら、終了してよいかユーザーに聞く（手動で操作を続けたい場合がある）。

**依存を増やさない実装のしかた**
- HTTP クライアントクレートを入れず、OS 標準の `curl` を絶対パスでサブプロセス実行する（mac 標準、Win10 1803 以降標準）。Windows の `curl.exe` は引数の非 ASCII 文字で失敗することがあるので、URL やパスに日本語を入れない。
- 簡単な通知・ファイル選択は GUI フレームワークなしで出せる（mac: `osascript`、Win: `mshta.exe`／VBS）。
- 定期的な再検査は Rust 側の `std::thread` で回し、`app.emit()` で UI へ通知する。処理がブロッキングなら async にする意味は薄い。
- インストール済みバイナリの arch は、PE ヘッダの Machine・Mach-O の cputype／fat header をファイルから読むだけで判定できる（レジストリ不要）。

**配布**
- 署名なし配布物への Gatekeeper／SmartScreen 警告は許容し、案内文で説明する方針（個人ツールに証明書の費用は見合わない）。
- 取得した実行ファイルを直後に起動するパターンは、AV のヒューリスティックに掛かりやすい。
- macOS 27（2026-09-14 公開）は Apple Silicon 専用で、Rosetta 2 の一般向けサポートは macOS 28 で終わる見込み。mac 向けは universal で出す。
- 「更新を忘れて古いバイナリを配る」取り違えは、生成の直前に毎回ビルドし直す作りにして仕組みで防いだ（2026-09-14 に実際に踏んだ）。

## ローカルソースを削除する前に

2026-11-11 に確認した範囲で、`sn_extension` のローカルにあって GitHub に無いもの:
- `src/docs/TODO.private.md`（613行。Marketplace との交渉記録。git 管理外）
- `.claude/`（スキル `patch-gen-gui-dev` と設定。スキルの要点は上に写した）
- ステージ済みで未コミットの `bun.lock`・`package.json`
- git 管理外のテスト補助ファイル（`test/prep.mjs`・`test/ui/runUI.mjs`・`test/int/*.js` など）と `tools/skynovel-Mac2Win`

ブランチは全て `master` にマージ済みで、`master` は `origin/master` と一致している。
