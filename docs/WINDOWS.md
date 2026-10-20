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
