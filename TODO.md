# TODO / 引き継ぎメモ

現時点で未完了・保留中の事項のみ。経緯や完了済みの作業は各ファイルの実装・コミットメッセージを参照（このファイルには書かない）。

**Shelf第3段階の検証が残っている**（詳細は「Shelf」節参照。2026-11-17時点）。

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

**mac のアプリケーションファイアウォールの初期設定について**: mac を親機にした直後は Win 子機からの着信が TCP 確立直後に切断される症状が出た（127.0.0.1 からの自己接続は成功）。`sudo socketfilterfw --add`/`--unblockapp` での許可登録・親機の再起動を行っても解消せず、最終的に**ファイアウォールを一旦まるごと無効化してから再度有効化・許可し直す**ことで解消した（許可リストへの登録自体は正しく反映されていたが、実行中バイナリへの反映に通常の再起動では不十分だった可能性がある。原因は未特定）。配布物を署名するまでは、初回セットアップ時にこの手順が要ることとして記録しておく。

**Win 実機で Avast CyberCapture による PC フリーズが発生（2026-11-11）。** リリース版（署名なし exe）を新しくビルドし直した直後に Win 機自体がフリーズし、ユーザーが PC を再起動する事態になった。影響で Win 子機・ゲームとも終了し、mac 親機との接続も切れた。

- **訂正**: 同日に一度「Avast の反応は確認した範囲では無し」と報告していたが、これは誤り。署名なしの新しい exe を作るたびに CyberCapture の対象になりうると考えること。
- 旧 Afterlife でも同種の報告があり（開発ビルドは再ビルドのたびにハッシュが変わる未署名バイナリのため、Avast が毎回ヒューリスティック解析する）、Lockstep でも実際に再現することが確認できた。
- 対応案（Avast の除外設定に `lockstep` の `target/` フォルダを加える）はあるが、ユーザー自身の判断が要るセキュリティ設定なので、Win 側セッションからは変更していない。**未対応。**
- 配布時の案内文で注意喚起する運用（「参考知見」節参照）は変わらないが、**開発中の頻繁な再ビルド自体もリスクである**ことが新しく分かった。

- **mac 子機での実ゲーム操作**: 未検証（mac に実ゲームを導入していない）。
- **Windows の DPI 100% 以外・複数モニタでのクリック着弾**: DPI 150%・単一モニタでは誤差1px以内を複数回確認済み（Win 側、2026-11-10・11）。100%・複数モニタは未検証。
- **取りこぼし検出の回帰確認（Lockstep）**: broadcastチャネルのlag等の実シナリオでのUI確認は未実施（`--autosend`の間隔では再現できず）。ユニットテスト（`detects_gap_as_missed`等）は通過済み。

## 踏んだ罠（記録）

- **踏んだ罠（配布物の作り方の問題）**: 最初に配っていた universal dmg は **11/10 にビルドした古いバージョン**で、Status/Reset/Ping プロトコル追加（11/11 の修正）を含んでいなかった。M6 からのクリック・キー適用自体（`ServerMsg::Click`/`Key`）は旧プロトコルでも成立するため正常に動くが、M6→親機への結果報告（`ClientMsg::Status`、旧版では`Result`という別形式）は親機側が新しい`ClientMsg` enumにデシリアライズできず`Err(_) => {}`で黙って握りつぶされ、親機画面は「受信待ち」のまま変化しなかった。クリック・キー自体は効くのに報告だけ届かないという紛らわしい症状になるので、**プロトコルを変更したら配布済みdmgは必ず作り直す**（`npm run tauri build -- --target universal-apple-darwin`）。dmg内バイナリの実際のビルド日時は `hdiutil attach` でマウントし `stat -f "%Sm" <バイナリ>` で確認できる。
- **mac の入力ソースが日本語（かな）だと、注入したキーが変換される**: M6 の TextEdit で "a" が "あ" に、Enter が改行でなく変換確定になった。Lockstep側の注入（物理キーコード送出）は正しく動いているが、IME が有効だと想定通りの文字にならない。英数入力に切り替えれば解消する。検証用に文字入力で確認するときは、対象機の入力ソースを先に確認すること。

## 未実装・検討

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
「参考知見」節参照。進め方は 第1段階モックランチャー → 第2段階生成ツール → 第3段階モックを実装で置き換え。
**第1段階（モックランチャー）・第2段階（生成ツール）は実装済み。**

## 次にやること（着手順。複数セッションをまたぐ想定）

この作業は長く続いており、再開するセッションが別人（別セッション）である可能性がある。
**着手前に必ずこの節と下の「検証待ち」「未実装・検討」を読むこと。**

1. **第3段階：実ランチャーで動かす検証**（新規コードはほぼ無い。生成ツール〈第2段階〉の
   出力を使った通し確認）。下の「検証待ち」に列挙した個別項目（実プロジェクトでのdmg生成・
   起動確認、R2実アップロード・GUI操作込みの確認、mac側の実インストーラー・実ゲーム操作）をこの順でなくてよいので埋める。他セッション
   （Win・M6）への依頼が複数残っている。
2. **GitHub Actionsでのスタブビルド導入**（第3段階で実際に困ってから着手するか判断する。
   今はローカルビルド＋Win実機→blues-sync手渡しで足りている）。`.github/`はこの
   リポジトリに一度も存在しない。`gh`のトークンに`workflow`権限が無いため、ワークフロー
   ファイルの初回pushで`gh auth refresh -s workflow`が要る可能性がある。
3. **秘匿の段階1：Cloudflare Workerによる所持証明付き短命URL**。`grantUrl`の口だけ用意済みで
   中身（Worker・nonce設計・リプレイ対策）は未設計。暗号設計が絡むため、着手時は
   設計だけ元の設計者（Fable）に戻すことを検討する。規模は第1・第2段階と同程度かそれ以上。
   Cloudflareをホスティングに使う場合のみ必要（秘匿の段階0のままでも動作上は問題ない）。

優先度の低い小粒の残件（ドラッグ＆ドロップ等）は「未実装・検討」節に個別記載。上の3つより後でよい。

## 検証待ち

- **ランチャーのアイコン指定**: macの生成・Winでの見た目（`ExtractAssociatedIcon`で取り出した画像の目視）は2026-11-18に確認済み。残りは、アイコン差し替え後のexe（`ランチャー_アイコン差替え.exe`相当）を**Winで実際に起動できること**と、エクスプローラー上の表示そのもの（取り出した画像でなく実画面）の確認。実物ランチャー（作品アイコン入り）の起動は確認済み。
- **R2（`bucket-of-blood`）には、大阪九龍条 v1.2.0 の実データを公開済み（2026-11-18、運用開始前）。** `games/sn_kowloon/{mac,win}/…`（実物のインストーラー）と、バケット直下の `manifest.json`・`manifest.json.sig`（本物の署名鍵で署名）。生成済みランチャーはこのマニフェストURLを埋め込んでいるため、消すとランチャーが動かなくなる。公開ベースURLで誰でも取得できる状態。バケット直下の既存 `sn_kowloon/`・`sn_osk_gitayu/` は旧パッチ配布ツールのもので別物なので触らない。公開ベースURLは `https://pub-254c788281004a619c71459f7368fae4.r2.dev`。生成ツールの状態・資格情報は `~/Library/Application Support/com.afterlife.shelf-gen/`、署名鍵は `~/.shelf-gen/signing-key.json`（紛失するとランチャーを全て作り直し）。
- **Shelf子機（Win/mac）での実ゲーム操作・実際のインストール上書きは未検証。** 第1段階は
  `dev-fixtures/` の偽インストールのみで検証済み（`SHELF_MAC_APPS_DIR`・`SHELF_CONFIG` で実機の
  `/Applications` を汚さずに検証できる）。**mac側はM6で試みたが実施できず（2026-11-17）**:
  M6のMacにSKYNovel/BlueSNovel製の実ゲームが入っていないため。対象の`appName`・実際の
  `setting.sn`のチェックサム・配布dmgを渡せば実施可能とのこと。

## 未実装・検討

- **段階1（Cloudflare Workerによる所持証明付き短命URL）**: マニフェストの `grantUrl` フィールドの
  口だけ用意済み（`shelf/core/src/manifest.rs`）。Worker本体・nonce設計・リプレイ対策は未設計。
  Cloudflareをホスティングに使う場合のみ必要（段階0のままでも動く）。
- **GitHub Actionsでのスタブビルド**: 生成ツールは現状ローカルビルド
  （`cargo tauri build --target universal-apple-darwin`を生成のたびに実行）。
  このリポジトリに`.github/`は一度も存在したことがない。`gh`のトークンに`workflow`権限が
  無いため、ワークフローファイルの初回pushは`gh auth refresh -s workflow`が必要になる可能性がある。
- **ドラッグ＆ドロップの実機確認（ユーザーの手動操作）**: 生成ツールの作品カードへのD&Dを実装
  （2026-11-17、`shelf/gen/ui/main.js`末尾。ビルド・構文チェックのみ通過。mac・Win両方で手動確認待ち）。フォルダ=プロジェクト、
  .dmg/.exe=最新版ペイン上ならその版・それ以外のカード上なら旧版インストーラー。ドロップ位置の
  判定（`onDragDropEvent`の座標と`devicePixelRatio`）は実際にドロップしないと確かめられない。

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
