#!/usr/bin/env bash
# Run through scripts/dev-env.sh after building the selected Apple target.
set -euo pipefail
cd "$(dirname "$0")/../.."
platform=${1:?usage: package.sh macos|ios-simulator}
case "$platform" in
    macos) rust_target=aarch64-apple-darwin ;;
    ios-simulator) rust_target=aarch64-apple-ios-sim ;;
    *) printf 'Unsupported platform: %s\n' "$platform" >&2; exit 1 ;;
esac
bundle="${CARGO_TARGET_DIR:-$PWD/target}/nsh-preview/$platform/Bex NSH Preview.app"
python3 - "$platform" "$bundle" "${CARGO_TARGET_DIR:-$PWD/target}/$rust_target/debug/agent-nsh" <<'PY'
import pathlib
import plistlib
import shutil
import sys

platform, destination, executable = sys.argv[1:]
bundle = pathlib.Path(destination)
info = {
    "CFBundleIdentifier": "com.bex.nsh-preview",
    "CFBundleExecutable": "agent-nsh",
    "CFBundleName": "Bex NSH Preview",
    "CFBundleDisplayName": "Bex NSH",
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": "0.1.0",
    "CFBundleVersion": "1",
    "NSMicrophoneUsageDescription": "録音した音声を接続先PCへ送り、文字起こししてメッセージに入力します。",
}
if platform == "macos":
    contents = bundle / "Contents"
    binary = contents / "MacOS" / "agent-nsh"
    info.update(LSMinimumSystemVersion="26.0", NSHighResolutionCapable=True)
    resources = contents / "Resources"
    resources.mkdir(parents=True, exist_ok=True)
    shutil.copy2("apps/desktop/assets/icon.icns", resources / "Bex.icns")
    info["CFBundleIconFile"] = "Bex.icns"
else:
    contents = bundle
    binary = bundle / "agent-nsh"
    info.update(
        CFBundleSupportedPlatforms=["iPhoneSimulator"],
        MinimumOSVersion="26.0",
        UIDeviceFamily=[1, 2],
        UILaunchScreen={},
        UIApplicationSupportsMultipleScenes=False,
        CADisableMinimumFrameDurationOnPhone=True,
    )
binary.parent.mkdir(parents=True, exist_ok=True)
shutil.copytree("apps/mobile/file-icons", (resources if platform == "macos" else contents) / "file-icons", dirs_exist_ok=True)
shutil.copy2(executable, binary)
with (contents / "Info.plist").open("wb") as output:
    plistlib.dump(info, output)
PY
if [[ $platform == ios-simulator ]]; then
    icon_info="${CARGO_TARGET_DIR:-$PWD/target}/nsh-preview/ios-icon-info.plist"
    xcrun actool apps/mobile/iosApp/Bex/Assets.xcassets --compile "$bundle" \
        --platform iphonesimulator --minimum-deployment-target 26.0 --target-device iphone \
        --target-device ipad --app-icon AppIconPreview --output-partial-info-plist "$icon_info" > /dev/null
    python3 - "$bundle/Info.plist" "$icon_info" <<'PY'
import plistlib
import sys
with open(sys.argv[1], "rb") as source:
    info = plistlib.load(source)
with open(sys.argv[2], "rb") as source:
    info.update(plistlib.load(source))
with open(sys.argv[1], "wb") as output:
    plistlib.dump(info, output)
PY
fi
codesign --force --sign - "$bundle"
printf '%s\n' "$bundle"
