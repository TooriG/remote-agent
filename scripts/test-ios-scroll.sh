#!/usr/bin/env bash
# Exercise native iOS components on macOS without a Simulator or Host.
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
    local fixture="apps/mobile/iosApp/UITests/Fixtures/$name.swift" key executable pending
    key=$({
        printf '%s\n' "$sdk" "$sdk_build" "$compiler" "$swift_target"
        shasum -a 256 "$0" "$@" "$fixture"
    } | shasum -a 256 | cut -d ' ' -f 1)
    executable="$output/$name/$key"
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
        mv -f "$pending/$name" "$executable"
        rmdir "$pending"
    else
        echo "Reusing unchanged $name build"
    fi
    "$executable"
}
run_test scroll-tests apps/mobile/iosApp/Bex/ConversationScrollObserver.swift
run_test text-view-tests apps/mobile/iosApp/Bex/ConversationTextLayout.swift
run_test app-diagnostics-tests apps/mobile/iosApp/Bex/AppDiagnostics.swift
