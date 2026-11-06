# NSH preview

Mac と iPhone で同じ Rust UI を動かす、NSH 0.4.0 の評価用アプリです。
このリポジトリの設定に合わせて macOS 26 / iOS 26 以降を対象にしています。
既存の `agent-core::ClientSessions` を使って Host に接続します。会話、下書き、送信、承認、モデル設定は共通 core の状態と操作を使います。接続先と保存領域、Keychain のサービス名は既存アプリから分離しています。
移植は進行中です。ブラウザー、ターミナル、ワークスペース、設定全体を含めた既存版との同等性はまだ達成していません。

Mac でビルドして起動:

```sh
scripts/dev-env.sh just nsh-macos
```

iPhone Simulator 用アプリをビルド:

```sh
scripts/dev-env.sh just nsh-ios-simulator
```

生成物は `target/nsh-preview/{macos,ios-simulator}/Bex NSH Preview.app` です。
Simulator には対象のデバイス ID を指定してインストールします。

```sh
scripts/dev-env.sh xcrun simctl install DEVICE_ID 'target/nsh-preview/ios-simulator/Bex NSH Preview.app'
scripts/dev-env.sh xcrun simctl launch DEVICE_ID com.bex.nsh-preview
```

会話切り替え、下書きの保持、送信・停止、Markdown の選択・コピー、モデル設定を確認できます。
ナビゲーション・ツールバー・一覧・検索・ボタン・メニューは UIKit／AppKit の標準コントロールを使います。
NSH の native component API はコントロールへのデータとイベントの接続に使います。
アプリアイコンは既存の素材、操作アイコンは OS のシステムシンボルです。音声の録音は AVFAudio、文字起こしは既存の Host の処理を使います。

稼働中 Host に接続せず Mac 版を検証するには、隔離 Host を起動します。プロバイダー監視用の実行ファイルも Host と同じ場所に必要です。

```sh
scripts/dev-env.sh cargo build --locked -p host-fixture -p agent-process --bins --target aarch64-apple-darwin
scripts/dev-env.sh env AGENT_FIXTURE_LOCAL_HOST=1 target/aarch64-apple-darwin/debug/agent-ui-fixture target/nsh-host-native target/aarch64-apple-darwin/debug/agent-codex-fixture target/nsh-host-native-port 8000
```

別のターミナルから、ビルド・パッケージ済みのアプリを同じ隔離領域で起動します。

```sh
scripts/dev-env.sh env AGENT_STATE_DIR="$PWD/target/nsh-host-native" AGENT_ISOLATED_HOST=1 'target/nsh-preview/macos/Bex NSH Preview.app/Contents/MacOS/agent-nsh'
```
