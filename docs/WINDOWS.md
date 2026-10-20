# Windows / Windowsで使う

BEX runs on Windows 11. The EXE installer includes the desktop app, Host, CLI,
provider supervisor, and Claude SDK bridge. You do not need Rust to install it.
The package is currently unsigned.

## Install and run

1. Download `bex-windows-x86_64-setup.exe` from https://getbex.dev.
2. Install Git for Windows, Node.js, and your agents (Codex and/or Claude Code); sign in with your own accounts. Install Microsoft Edge WebView2 Runtime if missing.
3. Run the EXE installer. It installs BEX, Host, supervisor, CLI and the Claude SDK together in `%LOCALAPPDATA%\Programs\BEX`, with a Start menu shortcut. No administrator privileges or ZIP extraction are required.
4. Open BEX from Start and pair a phone through its invitation flow.

Before updating, finish active tasks and exit BEX and its Host, then run the new installer. Account and conversation data remains outside the install directory and is not removed by uninstalling. These previews are unsigned and target x86-64 Windows 11. ZIP remains an optional developer artifact.

## Update Codex CLI

In BEX settings, select Codex and choose **最新版に更新** under **Codex CLI**.
BEX runs the selected Host's `codex update`, verifies the installed version,
and reports failures in the same panel. **状態を確認** also re-reads the installed
version after an update performed outside BEX. Finish active conversations and
restart BEX and its Host after updating so new sessions use the new executable.
The CLI must support `codex update`; otherwise update it using its installer.

## Build from source

Install the MSVC Rust toolchain and Visual Studio C++ Build Tools with the
Windows SDK, in addition to the runtime prerequisites above. Use the Rust
version pinned by the repository's Nix flake (as shown in the **Native clients**
toolchain job).

```powershell
./scripts/package-windows.ps1
# Faster local development package:
./scripts/package-windows.ps1 -Profile dev
```

Both profiles embed the Windows renderer's compiled shaders. The workspace
disables `debug_assertions` only for `gpui-pre-windows` in the development
profile, including its build script, so it compiles HLSL at build time. Keep
this override when building development previews:
GPUI's debug renderer otherwise loads shaders from the builder's Cargo registry
directory and fails on another PC with `Error creating DirectWriteTextSystem`
and a missing-path error. Runtime users do not need the SDK or Cargo sources.

Shader compilation uses the Microsoft Windows SDK C++ NuGet package pinned
in `tools/windows-fxc/manifest.json`. Both ordinary Cargo builds and CI use the
same verified x64 `fxc.exe` and `d3dcompiler_47.dll`; the package is downloaded
from Microsoft's official NuGet feed into `target/tool-cache/windows-fxc`.
The package and extracted files are checked with SHA-256. Missing or modified
cached files stop the build; installed SDKs, PATH and `GPUI_FXC_PATH` cannot
select a different shader compiler. Delete the affected version's cache to
restore it from the pinned package. Initial setup needs network access.

The vendored GPUI build script tracks the manifest, preparation script and all
HLSL inputs, so changing a pin or shader recompiles its bytecode. Windows CI
also checks rejection of missing/corrupted compiler files and identical shader
bytecode from two different source directories. This pins shader compilation;
it does not establish byte-for-byte reproducibility of the entire MSVC build.

**Update Windows shader compiler** checks the official NuGet feed every Sunday
at 04:00 Japan time (GitHub may delay scheduled runs). It selects the newest
listed stable package without downgrading, hashes its archive and matching x64 compiler
files, and opens or updates one PR on `automation/windows-fxc`. It explicitly
dispatches **Native clients** validation for the proposed pin; review that run
before merging the PR. Updates are not auto-merged, and ordinary builds never
query the latest version. The repository must allow GitHub Actions to create
pull requests; the updater does not approve them.

To check for an update manually, run the workflow from GitHub Actions, or run
`scripts/dev-env.sh pwsh -NoProfile -File tools/windows-fxc/update.ps1` locally.
The local command changes only the manifest when a newer stable version exists.

Install Inno Setup 6.3+ (or supply `-InnoCompiler path/to/ISCC.exe`).
The script builds desktop, Host, supervisor, and CLI from the same checkout,
then writes the EXE installer, optional ZIP, and SHA-256 checksums under `target/windows-packages/`
(or the Cargo-configured target directory). `-SkipBuild` packages already-built
executables and is intended for CI immediately after verification.

## 日本語

getbex.dev からWindows用EXEインストーラーを取得して実行します。BEX本体・Host・CLI・Claude SDKをまとめて配置し、スタートメニューにBEXを登録します。ZIPの展開やRustの導入は不要です。Git、Node.js、利用するエージェントとWebView2は別途必要です。

更新前に実行中のタスクを終え、BEXとHostを終了してから新しいEXEを実行してください。会話・アカウントのデータはインストール先の外に保存されます。対象はWindows 11・x86-64です。

Claude sign-in opens from the requesting BEX client. The Host suppresses the CLI's automatic browser launch on Windows and Unix, so the user receives only the manual authorization URL. After approving, copy the code from the web page into BEX and choose Connect.

## DeepSeek Harness

Install Node.js 22.19 or newer and `npm install -g @deepseek-ai/dsh@0.2.1-rc.2` on the Host PC. In BEX, open **設定 → エージェント → DeepSeek → APIキーを管理** (also available from the model picker). Choose DeepSeek, OpenRouter, NanoGPT, OpenAI, Anthropic, Gemini, Groq, Mistral or xAI, then enter and save its API key. **APIキーを取得** opens the selected provider’s key page. Keys are masked, stored on the connected Host separately from conversations, and never returned to clients. Saved keys can be replaced or removed individually; another provider’s key is preserved. Wait for active DSH tasks to finish before changing credentials. BEX enables the corresponding DSH routes, and their models appear in the model picker after saving. All devices connected to that Host can use these routes. Existing environment or Harness credentials remain available when BEX has no managed keys; BEX-managed credentials take precedence for its Harness process.

For NanoGPT, save its key under **NanoGPT** and choose **NanoGPT · DeepSeek V4.1 Flash** (or the Thinking variant). This route uses `https://nano-gpt.com/api/v1` and the canonical `deepseek/deepseek-v4.1-flash` model ID from NanoGPT's catalog. Reasoning choices come from Harness and use NanoGPT's wire values. Existing paired Android clients can select the route; preview 45 also refreshes its reasoning choices when selecting a model.

Choose **DeepSeek V4.1 Flash 1017** to use the official `deepseek-flash` route (currently V4.1-Flash-1017). Other models configured in Harness appear from its live catalog. BEX uses the official ACP interface for prompts, reasoning, tool results, permissions, cancellation and resuming the same native session after a Host restart. Additional messages wait until the current task finishes; fork is unavailable. Permission defaults belong to Harness; permission requests are answered in BEX.

For a separate installation, pass `host-daemon --deepseek C:\path\to\dsh\lib\bin.js`. `BEX_DEEPSEEK_NODE` selects its Node runtime. The standard Windows installation also detects a BEX-managed Harness under `%LOCALAPPDATA%\BEX\deepseek-harness`.

Host・Windows版・Android版は同じ対応版へ更新してください。DeepSeek用APIキーをPCのBEXで設定すると、そのHostへ接続するAndroid端末からも利用できます。

## Chromeからブラウザデータを取り込む

BEXのブラウザにある「Chromeから取り込む」から、次の操作ができます。

- パスワード: 「Chromeでパスワードを書き出す」でChromeを開き、Googleパスワードマネージャーの設定からCSVをダウンロードします。「パスワードCSVを取り込む」でそのファイルを選択してください。取り込み後は、書き出したCSVを削除してください。
- ブックマーク: メニューに表示されるChromeのプロファイルを選ぶと取り込めます。「ブックマーク」から開けます。
- ログイン: サイトを開き、「ログインを入力」からアカウントを選ぶと、表示中のログインフォームに入力します。送信は自分で行います。同じオリジン（スキーム・ホスト・ポート）の保存情報だけを使います。HTTPSに対応し、HTTPはローカル開発用のループバックのみ対応します。別オリジンのフレームやパスワード変更フォームには入力しません。
- 削除: 「取り込んだデータをすべて削除」でBEX内の取り込みデータを消せます。Chromeの元データは変更しません。

取り込んだパスワードとブックマークは、このWindowsユーザーのDPAPIで暗号化してローカルの`browser-import.dpapi`に保存します。Host・AIプロバイダーへの送信や端末間同期は行いません。再取り込みでは同じサイト・ユーザー名のパスワードを更新します。Windows版Chromeのアプリに紐づく暗号化を回避せず、パスワードはChromeの正式なCSV書き出しを利用します。Cookie・ログイン中セッション・パスキーの移行は対象外です。

## 使用量とリセット

利用上限のエラーに表示される「使用量・リセット時刻」から、設定のエージェント画面を開けます。使用量とローカル時刻のリセット予定を確認し、「使用量を再取得」で表示を更新できます。Codexには「追加クレジット・上限を管理」も表示され、BEXのブラウザでChatGPTの管理画面を開きます。ログイン中のアカウントを確認して操作してください。利用枠そのものをBEXから強制リセットする機能ではありません。提供元から取得した使用量には最大60秒のキャッシュがあります。
