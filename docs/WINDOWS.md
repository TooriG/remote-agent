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

Install Inno Setup 6.3+ (or supply `-InnoCompiler path/to/ISCC.exe`).
The script builds desktop, Host, supervisor, and CLI from the same checkout,
then writes the EXE installer, optional ZIP, and SHA-256 checksums under `target/windows-packages/`
(or the Cargo-configured target directory). `-SkipBuild` packages already-built
executables and is intended for CI immediately after verification.

## 日本語

getbex.dev からWindows用EXEインストーラーを取得して実行します。BEX本体・Host・CLI・Claude SDKをまとめて配置し、スタートメニューにBEXを登録します。ZIPの展開やRustの導入は不要です。Git、Node.js、利用するエージェントとWebView2は別途必要です。

更新前に実行中のタスクを終え、BEXとHostを終了してから新しいEXEを実行してください。会話・アカウントのデータはインストール先の外に保存されます。対象はWindows 11・x86-64です。
