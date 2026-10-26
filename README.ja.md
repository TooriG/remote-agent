# bex

[English](README.md) | [日本語](README.ja.md)

bexは、信頼できるコンピューター上のCodexとClaude Codeを、Macアプリ、iPhoneアプリ、Androidアプリ、またはヘッドレスCLIから操作するアプリです。Rust製のHostデーモンがエージェントのプロセスを管理します。すべてのクライアントは認証済みのirohストリームで共通のPostcard RPC型を使い、同じRustの`Store`へ操作を送ります。動作変更は[CHANGELOG.md](CHANGELOG.md)を参照してください。

irohは公開リリースを使い、netwatch 0.19.3は[UDPの再バインド回復](https://github.com/n0-computer/net-tools/pull/235)を含むフォークのコミットに固定しています。UDPの再バインド失敗時は、非同期I/Oを終了せず、100ミリ秒から5秒までの指数バックオフで再試行します。ソケットを明示的に閉じると回復処理を中止します。修正が含まれる上流リリースを採用したら、このパッチを削除します。

## 対応OS

Linux（CIの基準はUbuntu 24.04）とAndroid（最小はAndroid 8.0・API 26）を除き、BEXは一般提供されている最新のOSメジャーバージョンと、その安定版のマイナー・パッチリリースのみをサポートします。この最新メジャー方針では、ベータ、RC、旧メジャー、将来のメジャーは対象外です。Apple・Androidの最小配置バージョンで旧OSへのインストールを防ぎますが、新しいOSを実行時に強制停止する上限チェックはありません。

**2026-09-13**に確認した基準:

| プラットフォーム | 対応メジャー | ビルド・テストの基準 |
| --- | --- | --- |
| iPhone / iPad | iOS / iPadOS 26 | 安定版Xcode 26。アプリ、UIテスト、Rust・Swiftバインディングの配置先は26.0。iOS 26 Simulatorを使用 |
| macOSのデスクトップ・Host・CLI | macOS Tahoe 26 | Rustの配置先、アプリ・ヘルパーバンドルの最小OS、Swiftヘルパーの対象は26.0 |
| Android | Android 8.0以降（API 26以降） | 最小SDKは26、コンパイル・対象SDKは37。Nixのビルド・テストSDKも37 |
| Windowsのデスクトップ・Host・CLI | Windows 11、Windows Server 2025 | `windows-2025` CIでServer 2025上のビルド・テストを実行。Windows 11のUI受け入れテストは別途必要 |
| Linuxのデスクトップ・Host・CLI | Ubuntu 24.04をCI基準に使用 | GitHubホストの`ubuntu-24.04`と、固定した`nix develop .#native`環境 |

出典: [Appleのセキュリティリリース](https://support.apple.com/en-ca/100100)、[Android 17の公開](https://android-developers.googleblog.com/2026/06/Android-17.html)、[API 37のSDK設定](https://developer.android.com/about/versions/17/setup-sdk)、[Windowsクライアントのリリース](https://learn.microsoft.com/en-us/windows/release-health/windows11-release-information)、[Windows Serverのリリース](https://learn.microsoft.com/en-us/windows/release-health/windows-server-release-info)。Windowsの25H2・26H1などの機能更新は、新しいOSメジャーとは扱いません。

Android 8〜16ではAndroid 17専用のローカルネットワーク許可を要求しません。Android 17では、LAN接続を開く前に付近のデバイスへのアクセス許可を求めます。拒否した場合は再試行・設定ボタンを備えた許可画面に留まり、インターネット経由でも続行できます（LAN専用Hostには許可が必要です）。設定から許可を与えて戻るとアプリが開きます。インターネット経由の選択はActivityの再生成後も保持し、新規起動では許可を再確認します。保存済みHostと下書きは既存の保存領域に残ります。

新しいメジャーが一般提供されたら、提供元の公開を確認し、これらの基準をまとめて更新して、古い互換処理を削除します。対応を宣言する前にクライアントの受け入れテストを実行してください。SDKやCIランナーに含まれるだけのベータ・RCを採用してはいけません。この方針はMarkdown描画ライブラリの選択とは独立しています。

## Hostの起動

デスクトップの初回設定では、環境とAIを確認した後、Codex・Claude Codeの既存プロジェクトを選んで取り込めます。直近30日以内に使い、会話が3件以上あるGitリポジトリを初期選択します。取り込みは選んだフォルダーの登録で、ネイティブの履歴と会話IDは保持します。AIの設定と取り込みは後回しにでき、後から「設定 → 端末と接続」でも取り込めます。一部のAIの探索に失敗しても、他のAIの候補は残ります。画面の流れとClaude確認プロセスの設定は[T3 Codeの初回設定](https://github.com/pingdotgg/t3code/blob/4fef307dd0a0e9eb1a7bd610ed390b6764295a65/docs/user/welcome-wizard.md)を参考にしています。

会話にはGitと、利用可能なエージェントが最低1つ必要です。Codexは任意です。PATH上の`codex`を使い、見つからない場合はmacOSのChatGPT Desktop同梱版を使います。`--codex <path>`で明示した実行ファイルが最優先です。

Claudeとの会話は公式Agent SDKが実行します。Macアプリは署名済みのポータブルなNode.jsを同梱するため、Claude Codeがインストール済みならNodeやnpmを別途用意する必要はありません。既定ではアプリ内のNodeを使い、`BEX_NODE`で別の実行ファイルを明示できます。単体HostではPATHにNode.js 22以降を置くか、`BEX_NODE`で指定してください。Nixの開発環境はNode.jsを含み、Mac同梱用ランタイムのバージョンとチェックサムも固定します。SDKはnpmのlockfileに固定し、Hostへ同梱するコードを生成します。

```sh
scripts/dev-env.sh cargo build --locked -p bex-process --bin bex-provider-supervisor
scripts/dev-env.sh node scripts/install-claude-sdk.mjs target/debug
scripts/dev-env.sh cargo run -p host-daemon -- --name 'BEX Host'
```

| オプション | 内容 |
| --- | --- |
| `--state-dir` | 新しいHostの認証情報ディレクトリ。未指定なら記憶したディレクトリを使用（macOSの初期値は`~/Library/Application Support/app.bex.BEX/`）。通常Hostを2つ起動するための指定ではありません |
| `--codex-home` | 別のCodex保存領域を選択 |
| `--claude <path>` | Claude Codeの実行ファイル。既定値はPATH上の`claude`。デスクトップは設定済みの`BEX_CLAUDE`を渡します |
| `--relay-url <url>…` / `--no-relay` | 独自のirohリレー、または隔離テスト用のローカルアドレスのみを使用 |
| `--isolated --state-dir <directory>` | 明示的に分離した開発・テストHost。自身のディレクトリロックも保持します |

通常起動では、デスクトップとデーモンの状態ディレクトリが異なっていても、OSユーザーごとに1つのHostを共有します。プラットフォームのデータディレクトリに`host-instance.json`と短時間の探索ロックを置き、各Hostは稼働中ずっと自身のディレクトリをロックします。再起動後も同じHostの識別情報とディレクトリを使います。競合する起動は認証情報を作る前に拒否し、デスクトップは登録済みHostに認証して接続します。

同じコンピューター上のクライアントは、リレーとアドレス検索を無効にしたIPv4ループバックQUICで接続します。`host.ticket`にはループバックアドレスのみを含め、リモート用招待は通常のLAN・インターネット用エンドポイントを使います。どちらも同じ認証済みRPCとHostの状態を使用します。デスクトップは接続断でもStoreを保持し、現在のローカルHostのアドレスを再探索して、250ミリ秒から5秒の指数バックオフで復帰します。復帰時はHostの状態を読み直し、結果が不明な送信を自動再送しません。停止済みのHostをバックグラウンドの復帰処理で起動することもありません。

バックアップが必要で、Gitにコミットしてはいけないファイルは、識別鍵の`identity.keys`（64バイト、所有者のみアクセス可能）と、招待・許可リスト・リモートチケットを含む`trust.json`です。エラーは`logs/host.jsonl`（デスクトップは`logs/desktop.jsonl`）に追記し、5 MiBごとにローテーションして4世代を保持します。認証情報は可能な範囲で伏せます。

Codexのイベント処理終了は、プロセスの片付け前に`host.codex.event_stream_stopped`として記録します。停止原因、通信エラー・欠落イベント数、プロバイダーのインスタンス、最終処理シーケンス、停止要求の有無を含みます。要求された停止は`info`、予期しない終了は`error`です。会話本文は記録しません。

接続障害では`network.change`、`network.socket.rebound`、`network.socket.rebind_failed`、`network.socket.closed`、`network.endpoint.receive_failed`、QUICの`network.*.io_error`も記録します。固定したiroh・netwatch・noqから、エラー種別と数値のOSエラーコードを記録し、依存ライブラリのメッセージ、アドレス、ペイロードは保存しません。同じ障害は操作ごとに30秒で1件までとし、コードの変更、ネットワーク変更後、再バインド成功後の障害は直ちに記録します。`previous_suppressed`は省略した反復回数です。ネットワーク変更・再バインドで制限をリセットした場合は、操作をまたいだ省略回数を示します。接続性能タイムラインを無効にしても、これらの記録は残ります。調査前の再起動は避け、Host・Desktop双方のローテーション済みログ、時刻、PID、ビルドリビジョンを確認してください。

### ヘッドレスLinuxとiPhoneのペアリング

固定したLinux環境でHostと監視用実行ファイルをビルドします。

```sh
nix develop .#native --command cargo build --locked --release -p host-daemon -p bex-process -p agent-cli
target/release/host-daemon --name 'Linux development'
```

別のシェルから、同じOSユーザーとして管理コマンドを実行します。稼働中Hostを探索して認証し、別のデーモンを起動したり認証情報を書き換えたりしません。

```sh
target/release/host-daemon status
target/release/host-daemon invite
# qrencodeがある場合、招待をQRコードとして表示:
target/release/host-daemon invite | qrencode -t ANSIUTF8
# statusに表示されたノードIDで端末のアクセスを取り消す:
target/release/host-daemon revoke <node-id>
```

iPhoneで**PC一覧 → PCを追加 → QRコードを読み取る**を開くか、**QRの内容を手入力**に招待JSONを貼り付けます。招待の有効期限は既定で7日、登録できる新規端末は1台です。Hostの起動時に`--invitation-days <1–90>`を指定すると、新しく発行する招待の期限だけを変更できます。端末ごとに別の招待を発行し、出力をログやGitへ保存しないでください。登録済み端末は招待の期限後も再接続できます。Hostの識別情報と登録済み端末は再起動後も保持します。隔離開発Hostでは、`invite`・`status`の前にも同じ`--isolated --state-dir <directory>`を指定します。

インターネット経由の接続には既定のirohリレーを有効にします。Git、エージェントの実行ファイル、ChromiumがPATH上にある開発ユーザーでサービスを動かし、そのユーザーのエージェントアカウントを認証してください。`bex-provider-supervisor`は`host-daemon`と同じディレクトリに置きます。iPhoneから操作するのはLinux上のファイル、ターミナル、ブラウザーです。プロジェクトのタスク開始時はLinux側のチェックアウトを選びます。systemdでは`KillSignal=SIGINT`を使い、Hostがプロバイダーを正常終了できるようにします。

信頼できる共有環境では、`codex login --with-api-key`の標準入力でチーム用APIキーを設定できます。Hostはこれを**API key**アカウントとして検出し、再起動後も選択を保持します。API料金はChatGPTのサブスクリプション使用量と別なので、サブスクリプションの残量は表示しません。共有Codex設定で`forced_login_method = "api"`を指定するとAPI認証を維持できます。ペアリングした全端末がHostのアカウントと認証情報を共有し、タスクごとのワークツリーでソース変更を分離します。

ワークツリー設定の`bex-worktrees.json`と、プロジェクトに属さないチャットの`bex-chats/`は、通常`$CODEX_HOME`または`~/.codex`のCodexプロジェクト状態の隣に保存します。新規ワークツリーの既定パスは`<original-repository>/.worktree/session-XXXXX/<repository-name>`です。独自ディレクトリを設定すると`.worktree`部分を置き換えます。別のワークツリーから開始しても、チェックアウトのフォルダー名はリポジトリ名を使います。既存ワークツリーは移動しません。既定の`.worktree`はGitのローカル`info/exclude`で除外します。設定画面はMacの**設定 → ワークツリー**、iPhoneの**タスク一覧 → … → 設定 → ワークツリー**です。

iPhoneから保存済みPCを削除するには、**タスク一覧 → PC一覧 → 接続を解除**を開いて確認します。そのiPhoneの認証鍵を削除し、再起動後も再接続しません。Hostの会話データは保持します。Macの**設定 → 端末と接続 → 接続を解除**は、端末のアクセスを取り消して接続を閉じます。再接続には再ペアリングが必要です。iPhoneでPCを削除しても、Macにある以前の端末エントリーは削除しません。

## Windows版の利用

Windows 11では、ビルド済みEXEインストーラーを使えばRustのインストールは不要です。
必要なランタイム、リリース配布、検証済みCI開発版、ソースビルドの手順は
[Windows版の利用手順](docs/WINDOWS.md)を参照してください。

## クライアントのビルド

```sh
# Mac配布用（署名証明書が必要。BEX_CODE_SIGN_IDENTITYで選択）
scripts/dev-env.sh just build-desktop-macos && open target/release/bex.app

# iPhone（iOS 26）: Simulator用ライブラリをビルドしてXcodeを開く
scripts/dev-env.sh scripts/build-agent-ios.sh simulator
open apps/mobile/iosApp/Bex.xcodeproj

# Android 8.0以降（ビルドSDKは37）
scripts/dev-env.sh ./gradlew :apps:mobile:assembleDebug
```

別アプリとして配布するiOS開発版は、[配布手順](docs/qa/IOS_DEV_RELEASE.md)を参照してください。

Rustのソースを変更したら、iOS用ライブラリも再ビルドしてください。デスクトップの下書きとログは`BEX_STATE_DIR`を使いますが、Host探索で別の認証情報ディレクトリを選ぶことがあります。デスクトップを隔離するには`BEX_ISOLATED_HOST=1`と`BEX_STATE_DIR`の両方が必要です。個人のプロバイダー状態をテストが読み込まないよう、別のCodexホームかフィクスチャ実行ファイルも使ってください。

編集中は`scripts/dev-env.sh just check -p agent-core`で、実行ファイルを生成せずにRustを確認できます。`-p`には変更したクレートを指定してください。

`scripts/dev-env.sh just dev`は、Hostとデスクトップを同じdebugビルドで`target/debug/bex.app`にまとめ、プロバイダーのアカウントと会話履歴を共有する別のローカルHostを起動します。配布用は`target/release/bex.app`に保存します。Macの両ビルドコマンドは`dev`または`release`を指定でき、既定は`release`です。再ビルド前に実行中タスクを確認し、古い開発Hostを停止してください。ウィンドウを閉じるだけではHostは停止しません。同じ会話を2つのHostで同時に実行しないでください。

## 会話の操作

プロジェクトの`…`メニューから、ピン留め、表示名・セクションの変更、アーカイブ、非表示を操作できます。プロジェクト見出しをドラッグして並べ替え、チャットをプロジェクトへドロップして一覧上の配置を変更できます。端末ごとの表示設定であり、会話の実行ディレクトリやファイルは変更しません。

iPhoneでは会話のグリッドボタンから作業画面を開き、ターミナル・ブラウザー・ファイルのタブを選びます。ターミナル画面を閉じたり接続を切ったりしてもシェルは残り、「終了」で終了できます。Hostの再起動ではシェルが終了します。ブラウザーは会話のエージェントと同じHost上のページを操作し、ログイン用プロファイルはHost再起動後も保持しますが、開いていたタブの復元は保証しません。

Mac・iPhoneでは、アシスタントの文章を選択して下書きに引用したり、サイドチャットで質問したりできます。iPhoneのサイドチャットを閉じると、元の会話と下書きへ戻ります。Macには右クリックのコピー・Google検索、自分のメッセージのホバーによるコピー・入力欄への戻しもあります。コマンドの実行内容は実行中・再表示後とも最初は折り畳み、明示的に展開した状態は保持します。

Mac・iPhoneでは、枠のないFast、モデル名、推論強度の操作をマイク・送信の直前に置きます。モデル名を押すと、エージェント・アカウントの行と、検索できるモデル一覧を開きます。アカウント行は報告された週次使用枠を表示し、切り替えや、追加・ログイン・確認付きサインアウトの管理画面へ進めます。設定からも同じ管理画面に入れます。アカウント変更では、対応するモデル設定、本文、添付を保持したまま一覧を更新します。認証とプロバイダーごとのアカウント選択はHostが所有し、接続済みクライアントで共有します。利用可能なエージェントとモデルは接続先Hostが報告するものを使います。Claudeではログインページを開き、返された認証コードをBexへ貼り付けます。

### Claude Code

追加アカウントはHostの状態ディレクトリ内に独立したClaude設定ディレクトリを持ち、認証情報はClaude Codeが管理します。各`projects`ディレクトリはネイティブの会話記録へリンクするため、アカウント変更で履歴は失われません。選択はHostの再起動後も保持し、次のターンから適用します。実行中のターンは元のアカウントで完了します。

Hostに通常のパッケージ管理でClaude Codeをインストールし、BexからClaudeのサブスクリプションアカウントを追加するか、Host上の既存の`claude auth login`を使います。新しい会話を作成し、入力欄のモデル選択で**Claude · …**を選びます。モデル名と対応する推論強度は、インストールしたCLIの初期化応答から取得し、選択中アカウント用にキャッシュします。アカウント変更時にキャッシュを無効化します。Claude実行ファイルがない場合もCodexモデルは表示し、プロバイダーの失敗は利用可能なモデルとともに報告します。不完全な一覧でも保存済みモデルの選択を保持し、新規下書きは利用可能なモデルを選びます。

Hostは変更していないCLIを、ストリーミングJSON入出力と独自の権限コールバックで動かします。Claudeが認証情報を管理し、bexはサブスクリプショントークンをCodexへコピーしたり、Anthropicの推論APIを直接呼んだりしません。Claudeのターンにはサブスクリプション認証が必要で、APIキー認証・未認証では送信前に拒否します。Hostの環境でAPIキーを設定している場合は、その上書きを外してください。Codexのアカウント選択はCodexにのみ適用します。Anthropicの[認証](https://code.claude.com/docs/en/authentication)、[プログラム実行](https://code.claude.com/docs/en/headless)、[組み込み条件](https://code.claude.com/docs/en/legal-and-compliance)を参照してください。

テキスト、PNG・JPEG・GIF・WebP画像、ファイル参照、ツール承認、ユーザーへの質問、中断、後続ターンは、Codexと同じStore・認証済み接続を使います。作業ディレクトリ選択と自動ワークツリー設定も共通です。永続的な会話の唯一の保存元はClaudeのネイティブ記録です。Hostは`--claude-home`、`CLAUDE_CONFIG_DIR`、または`~/.claude`を推論実行なしで読み、Bex独自の会話記録は書きません。クライアントを切断しても実行は続き、再接続ではイベントを再送せず、現在有効な未回答の要求を含む最新の状態を開きます。

会話中もモデル選択からCodex・Claude・DSHを切り替えられます。現在の応答と未回答の確認を終え、次のメッセージを送信すると、同じ会話・作業場所で続行します。以前使ったAIへ戻ると、そのAIのネイティブ会話を再利用し、まだ見ていない履歴を渡します。長い履歴は最初の依頼と最近のやり取りを優先し、省いた内容はAIがHostの履歴コマンドで参照できます。過去の添付や思考・ツールの内部状態は移しません。本文は引き続き各AIのネイティブ記録を使い、切替先の対応と投入した文脈を`bex-conversations.json`へ保存します。Hostとクライアントを同じリビジョンから更新してください。

ClaudeからCodexへの委任、Claude自体のフォーク・サイドチャットは未実装です。Codexの起動失敗・終了でも、Host、Claudeの実行、ペアリング、ファイル、作業場所の選択は停止しません。HostはCodexと独立してPTYを所有し、端末に属するシェルはクライアントの切断後も保持します。音声入力とCodexアカウント操作にはCodexが必要です。Codexの会話活動を確認できない場合はワークツリー削除を禁止します。CLI連携はClaude Code 2.1.266で検証しています。

Macで会話内のファイルリンクを開くと、FilesパネルにHost上の親ディレクトリとエディターを表示し、未保存の編集とリビジョン確認を保持します。Side Chatは独自パネルでファイル・差分を表示し、下書きを保ったまま会話へ戻れます。

Macで**ブラウザ → Chromeから取り込む**を開き、Chromeのプロファイルを選ぶと、CookieをBexのブラウザーへコピーします。macOSが**Chrome Safe Storage**のキーチェーンアクセスを求めたら許可してください。Chromeのデータベースは変更せず、ドメイン・名前・パスでCookieを統合し、保存を確認してから現在のページを再読み込みします。永続CookieはBex再起動後も期限を保持し、セッションCookieはセッション内のみ有効です。継続同期ではなく1回のコピーなので、ログインを更新するには再取り込みします。期限切れ・トップレベルサイトで分割されたCookieは除外して件数を表示します。ローカルストレージや端末に紐付く認証など、Cookie以外の状態はコピーしないため、Bex内でのログインが必要なサイトもあります。

Cookie取り込みはmacOS Chromeのデータベースバージョン23・24に対応します。読み取り・属性の回帰確認は`scripts/dev-env.sh cargo test --locked -p bex-desktop --bin bex-desktop chrome_tests`で実行します。macOS 26の`scripts/dev-env.sh just macos-e2e`は、使い捨てWebKit保存領域で、実際のBrowser画面、暗号化フィクスチャ、アクセス拒否後の再試行、認証付きHTTP、HttpOnly保護、新規プロセスでの永続性を検証します。固定したlb-wryの`set_cookie`でドメインの範囲が失われる問題も記録しており、そのため取り込みはWebKitのネイティブCookieストアを直接使います。

デスクトップの会話一覧・タイトルはメッセージ送信とターン完了後に更新します。実行ヘッダーは経過時間と最新の操作を表示し、コマンド出力にはコピー操作があります。変更要約・ファイルを押すと差分を開き、ファイル選択や長い未変更部分の展開ができます。音声入力中は、キャンセル・停止・送信付きの波形を表示します。Escapeで録音を中止して下書きを保持します。

Macの設定は、Bexが作成したワークツリーと会話を**作成済みのワークツリー**に表示します。削除前に別の会話へ移動し、削除を確認します。実行中ターン、未回答の要求、未確定の送信、追跡中ターミナル、変更済み・未追跡・無視対象ファイル、ワークツリーロック、detached HEADがある場合はHostが拒否します。削除後もブランチ、会話履歴、一覧のエントリーを保持します。次のメッセージ送信時は、元リポジトリのローカル`main`から作った新しいブランチで、同じパスへ再作成します。永続化するのはワークツリーと元プロジェクトのパスのみで、以前のブランチ・削除状態は保存しません。

**マージ済みを自動削除**は既定で無効です。有効にするとHostが毎分確認し、作成時点より先のコミットがあり、現在のHEADがローカル`main`に含まれる、未使用で変更のないBexワークツリーを削除します。プロバイダーの履歴へまだ現れていない実行中セッションも含め、同じ保護条件を適用します。使用中のものは後で再確認し、新しい未統合コミットがあれば対象から外します。活動・Git状態を確認できなければ保持します。ブランチ・会話は残り、次のメッセージで再作成できます。

音声入力は、停止時に認識した文章を下書きへ追加し、送信時には下書き・添付とともに送ります。文字が認識されなかった正常終了は静かに終わり、送信を選んでいても下書き・添付を保持します。録音、接続、不正な応答、文字起こしの失敗はエラーを報告しますが、下書きは破棄しません。

## ヘッドレスCLI

```sh
agent-cli --ticket <endpoint-ticket> --identity-file <32-byte-key> [--invitation <uuid>] list
agent-cli <connection> send <thread-id> <text> --client-message-id <id> [--model m] [--provider codex|claude|deepseek] [--effort e]
agent-cli <connection> approve 7 --decision 2               # 数値の要求ID
agent-cli <connection> approve '"request-id"' --decision 2  # 文字列の要求ID
```

ローカルのフィクスチャではチケットの代わりに`--stdio <fixture-executable>`を使います。結果はJSONとして標準出力へ、エラーは標準エラーへ出力します。

## Agent peer CLIとスキル

[`tools/agent-peer`](tools/agent-peer)は、Claude・Codexへの相談CLI、スキル、テスト、単独Nixパッケージの共有ソースです。BEX側の利用箇所とともにここで開発します。このディレクトリだけでもmacOS・Linuxでビルドできます。

```sh
nix profile install .#agent-peer
agent-peer-install-skills
```

公開リポジトリ`ttizze/agent-peer`には、このディレクトリだけをMITライセンスで含めます。コミット済みリビジョンをsubtree splitで公開し、BEXの残りのソース・履歴を除外します。

```sh
peer_commit=$(git subtree split --prefix=tools/agent-peer HEAD)
git push git@github.com:ttizze/agent-peer.git "$peer_commit:refs/heads/main"
```

変更はここで行い、subtreeを再公開します。VPSには`nix profile install github:ttizze/agent-peer/<commit>`で固定コミットを入れ、エージェントのサービスユーザーで`agent-peer-install-skills`を実行します。プロバイダーCLIと認証は各コンピューター内に保持します。詳細は単独パッケージのREADMEを参照してください。

## 検証

```sh
scripts/dev-env.sh just unit-tests                  # 全ユニットテスト。Simulator・エミュレーターは起動しない
scripts/dev-env.sh cargo test --workspace           # 振る舞いコーパスを含むRustテスト
scripts/dev-env.sh just iroh-e2e                    # 隔離iroh接続で実デーモンを検証
scripts/dev-env.sh just android-e2e                 # Android 17: 接続許可、Markdown、永続化、Host復帰
scripts/dev-env.sh just ios-e2e [TestMethod…]        # フィクスチャHostに対するSimulator XCUITest
scripts/dev-env.sh just conversation-ui            # 選択、サイドチャット、実行表示の回帰確認
scripts/dev-env.sh just macos-e2e                  # ネイティブBrowserの認証と永続化
scripts/dev-env.sh just quality [apple|rust|kotlin|swift]  # 既定はMac Host・デスクトップとiPhone
```

Claudeの契約テストは`scripts/dev-env.sh cargo test --locked -p host-fixture --test claude`です。先に監視用実行ファイルをビルドしてください。決定的に動作する外部CLI境界と、実際のStore・iroh・Hostルーティング・ネイティブ会話ファイル・隔離Git環境を使います。Claude Code 2.1.266の匿名化した記録でもネイティブ形式との互換性を確認します。任意実行の`live_claude_subscription_completes_and_resumes_through_store_and_host`は実際の認証済みCLIを使います。`BEX_LIVE_CLAUDE_PROGRAM`に絶対パスを設定し、`-- --ignored --exact`を付けて実行すると、サブスクリプションでの推論、Host再起動後の再開、中断、中断後の入力を確認できます。

開発・統合の方針は[AGENTS.md](AGENTS.md)を参照してください。プラットフォームの受け入れ検証はNative clients CIが担当し、起動条件、対象選択、キャッシュ、ランナーは[native.yml](.github/workflows/native.yml)で管理します。

開発・テストビルドは、全変数のデバッグ情報を含めず、ファイル名・行番号付きバックトレースを保持します。デバッガーで変数が必要なら`CARGO_PROFILE_DEV_DEBUG=full`を指定してください。品質チェックではRustのインクリメンタルコンパイルを無効にし、通常のローカルビルドのインクリメンタル状態はKacheが管理します。

Nixシェルは固定したKacheでワークツリー間のビルド結果を再利用します。`KACHE_CACHE_DIR`でキャッシュ先を指定でき、`bex.buildRoot`設定時の既定値は`<buildRoot>/.kache`です。`scripts/dev-env.sh kache stats --last-build`で再利用、`scripts/dev-env.sh kache explain`でミスの原因を確認できます。キャッシュの方針は[.kache.toml](.kache.toml)で管理し、CIは独自のCargoキャッシュを使います。

接続時間は`scripts/dev-env.sh cargo xtask connection-diagnostics`で解析できます。対象を絞った検証コマンドは`scripts/dev-env.sh just --list`で確認できます。iOSの受け入れ検証は`BEX_IOS_TEST_WORKERS=1`〜`10`で並列数を指定できます。ランナーはワークツリー間で実行を直列化し、結果を`target/qa`へ残します。ビルドキャッシュの詳細は[scripts/dev-env.sh](scripts/dev-env.sh)と各ネイティブランナーで管理します。`agent-ffi/bindgen`機能はバインディング生成時にだけ有効にしてください。

新規ワークツリーのビルド出力を外付けディスクへ置くには、マウント済みディスクにディレクトリを作り、`git config --local bex.buildRoot /absolute/path/to/builds`を設定します。このローカル設定は全ワークツリーで共有します。`scripts/dev-env.sh`は新しい`target`を外付け上の専用ディレクトリへのシンボリックリンクにし、Cargo・Xcode・生成バインディング・キャッシュの既存パスを維持します。既存`target`は移動しません。移す場合は使用していない間に移動して元の場所をリンクへ置き換えてください。指定ディレクトリは事前に存在する必要があり、ディスク取り外しなどで利用できなければコマンドは失敗します。`git config --local --unset bex.buildRoot`で設定を削除すると、その後の新規ワークツリーはローカル出力へ戻ります。

`just quality`は未使用のビルド出力を整理します。`scripts/dev-env.sh just clean-builds --dry-run`で計画を確認でき、保持条件と使用中ビルドの保護は[整理処理](crates/xtask/src/build_cleanup.rs)で定義しています。バックアップや検証記録はCargoの`debug`・`release`ディレクトリの外へ保存してください。

Simulator・フィクスチャの実行では、実機、本番キーチェーン、カメラ、実際のCodexアカウントは検証できません。

## GitHubの保守

[mainのルールセット](.github/main-ruleset.json)に、GitHubへ適用する保護設定を記録しています。mainへの直接pushを許可します。任意のPRでは、Bexのマージ済みワークツリー整理でコミットの祖先関係を保つため、マージコミットを使い、squash・rebaseによるマージは無効にしています。

[Dependabot](.github/dependabot.yml)で更新対象、スケジュール、グループ、待機期間を管理します。依存関係の変更とネイティブCIの結果を確認してから統合してください。

Lucideの更新は[可視化ランタイムのREADME](crates/host-daemon/src/visualize/README.md)のコマンドで生成し、同READMEのバージョン・取得元・チェックサムも更新してください。npmのmanifest・lockfileと一緒に、生成したバンドル・ライセンスをコミットします。CIは再生成して古いバンドルを検出します。リポジトリ内へ取り込んだRust・Androidソースと、固定したnetwatchの回復パッチは、上流の変更を手動で確認する必要があります。

Dependabotの脆弱性通知・セキュリティ更新、秘密情報のスキャン・push保護を有効にしています。Actionsは完全なコミットSHAに固定し、既定トークンは読み取り専用で、自分のPRを承認できません。脆弱性の非公開報告方法は[SECURITY.md](SECURITY.md)を参照してください。
