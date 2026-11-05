#!/usr/bin/env bash
# Exercise UIKit scroll geometry on macOS without a Simulator or Host.
set -euo pipefail
cd "$(dirname "$0")/.."
sdk=$(xcrun --sdk macosx --show-sdk-path)
sdk_build=$(xcrun --sdk macosx --show-sdk-build-version)
compiler=$(xcrun swiftc --version)
swift_target="$(uname -m)-apple-ios26.0-macabi"
target=$(cargo metadata --no-deps --format-version 1 | jq -er .target_directory)
output="$target/qa/ios-scroll/builds"
mkdir -p "$output"
run_test() {
    local name=$1
    shift
    local fixture="apps/mobile/iosApp/UITests/Fixtures/$name.swift" key bundle executable pending
    key=$({
        printf '%s\n' "$sdk" "$sdk_build" "$compiler" "$swift_target"
        shasum -a 256 "$0" "$@" "$fixture"
    } | shasum -a 256 | cut -d ' ' -f 1)
    bundle="$output/$name/$key.app"
    executable="$bundle/Contents/MacOS/$name"
    if [[ ! -x $executable ]]; then
        mkdir -p "$output/$name"
        pending=$(mktemp -d "$output/$name/.build.XXXXXX")
        if ! xcrun swiftc -sdk "$sdk" -target "$swift_target" \
            -F "$sdk/System/iOSSupport/System/Library/Frameworks" \
            -L "$sdk/System/iOSSupport/usr/lib" \
            "$@" "$fixture" -o "$pending/$name"; then
            rm -rf "$pending"
            return 1
        fi
        # UIApplication needs an application identity even in a standalone Catalyst fixture.
        cat > "$pending/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>app.bex.qa.$name</string>
<key>CFBundleExecutable</key><string>$name</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleName</key><string>Bex UIKit tests</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSUIElement</key><true/>
</dict></plist>
PLIST
        mkdir -p "$bundle/Contents/MacOS"
        mv -f "$pending/Info.plist" "$bundle/Contents/Info.plist"
        mv -f "$pending/$name" "$executable"
        rmdir "$pending"
    else
        echo "Reusing unchanged $name build"
    fi
    "$executable"
}
run_test scroll-tests apps/mobile/iosApp/Bex/ConversationList.swift apps/mobile/iosApp/Bex/ConversationListLayout.swift
run_test text-view-tests apps/mobile/iosApp/Bex/ConversationTextLayout.swift
