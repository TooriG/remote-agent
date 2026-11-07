#!/usr/bin/env bash
# Build native Apple views and check their shared display contract on Mac.
set -euo pipefail
cd "$(dirname "$0")/../.."
platform=${1:?usage: build-native.sh macos|ios-simulator}
case "$platform" in
    macos) sdk=macosx; triple=arm64-apple-macosx26.0; bundle_platform=MacOSX ;;
    ios-simulator) sdk=iphonesimulator; triple=arm64-apple-ios26.0-simulator; bundle_platform=iPhoneSimulator ;;
    *) printf 'Unsupported platform: %s\n' "$platform" >&2; exit 1 ;;
esac
build_root="${CARGO_TARGET_DIR:-$PWD/target}/nsh-apple/$platform"
sdk_path=$(xcrun --sdk "$sdk" --show-sdk-path)
swift_arguments=(--package-path apps/nsh/apple --scratch-path "$build_root" --cache-path "${CARGO_TARGET_DIR:-$PWD/target}/tool-cache/swiftpm"
    --configuration debug --jobs 2 --triple "$triple" --sdk "$sdk_path" --force-resolved-versions)
xcrun swift build "${swift_arguments[@]}" --product BexNshNative
if [[ $platform == macos ]]; then
    xcrun swift test "${swift_arguments[@]}"
fi
product=$(xcrun swift build "${swift_arguments[@]}" --product BexNshNative --show-bin-path)
framework="${CARGO_TARGET_DIR:-$PWD/target}/nsh-preview-native/$platform/BexNshNative.framework"
mkdir -p "$framework"
cp "$product/libBexNshNative.dylib" "$framework/BexNshNative"
xcrun install_name_tool -id '@rpath/BexNshNative.framework/BexNshNative' "$framework/BexNshNative"
python3 - "$framework/Info.plist" "$bundle_platform" <<'PY'
import plistlib
import sys

with open(sys.argv[1], "wb") as output:
    plistlib.dump({
        "CFBundleIdentifier": "com.bex.nsh-native",
        "CFBundleName": "BexNshNative",
        "CFBundleExecutable": "BexNshNative",
        "CFBundlePackageType": "FMWK",
        "CFBundleVersion": "1",
        "CFBundleShortVersionString": "0.1.0",
        "CFBundleSupportedPlatforms": [sys.argv[2]],
        "MinimumOSVersion": "26.0",
    }, output)
PY
codesign --force --sign - "$framework"
