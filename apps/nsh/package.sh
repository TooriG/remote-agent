#!/usr/bin/env bash
# Package the iPhone Simulator preview after building agent-nsh for it.
# The Mac app is packaged by scripts/build-macos.sh desktop.
set -euo pipefail
cd "$(dirname "$0")/../.."
target=${CARGO_TARGET_DIR:-$PWD/target}
bundle="$target/nsh-preview/ios-simulator/Bex NSH Preview.app"
python3 - "$bundle" "$target/aarch64-apple-ios-sim/debug/agent-nsh" \
    "$target/nsh-native/debug/ios-simulator/BexNshNative.framework" <<'PY'
import pathlib
import plistlib
import shutil
import sys
import tempfile

def copy_file(source, destination):
    # A signed image may still be mapped by a previous launch. Give each
    # replacement its own inode so the new signature is validated.
    destination = pathlib.Path(destination)
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as pending:
        path = pathlib.Path(pending.name)
    try:
        shutil.copy2(source, path)
        path.replace(destination)
    finally:
        path.unlink(missing_ok=True)
    return str(destination)

destination, executable, native = sys.argv[1:]
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
    "CFBundleSupportedPlatforms": ["iPhoneSimulator"],
    "MinimumOSVersion": "26.0",
    "UIDeviceFamily": [1, 2],
    "UILaunchScreen": {},
    "UIApplicationSupportsMultipleScenes": False,
    "CADisableMinimumFrameDurationOnPhone": True,
}
binary = bundle / "agent-nsh"
binary.parent.mkdir(parents=True, exist_ok=True)
frameworks = bundle / "Frameworks"
frameworks.mkdir(parents=True, exist_ok=True)
shutil.copytree(native, frameworks / "BexNshNative.framework", dirs_exist_ok=True, copy_function=copy_file)
copy_file(executable, binary)
with (bundle / "Info.plist").open("wb") as output:
    plistlib.dump(info, output)
PY
icon_info="$target/nsh-preview/ios-icon-info.plist"
xcrun actool apps/mobile/iosApp/Bex/Assets.xcassets --compile "$bundle" \
    --platform iphonesimulator --minimum-deployment-target 26.0 --target-device iphone \
    --target-device ipad --app-icon AppIconDev --output-partial-info-plist "$icon_info"
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
codesign --force --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
printf '%s\n' "$bundle"
