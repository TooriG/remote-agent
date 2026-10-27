#!/usr/bin/env bash
# Exercise UIKit scroll geometry on macOS without a Simulator or Host.
set -euo pipefail
cd "$(dirname "$0")/.."
sdk=$(xcrun --sdk macosx --show-sdk-path)
target=$(cargo metadata --no-deps --format-version 1 | jq -er .target_directory)
output="$target/qa/ios-scroll"
mkdir -p "$output"
run_test() {
    local name=$1
    shift
    xcrun swiftc -sdk "$sdk" -target "$(uname -m)-apple-ios26.0-macabi" \
        -F "$sdk/System/iOSSupport/System/Library/Frameworks" \
        -L "$sdk/System/iOSSupport/usr/lib" \
        "$@" "apps/mobile/iosApp/BexUITests/Fixtures/$name.swift" \
        -o "$output/$name"
    "$output/$name"
}
run_test scroll-tests apps/mobile/iosApp/Bex/ConversationScrollObserver.swift
run_test text-view-tests apps/mobile/iosApp/Bex/ConversationTextLayout.swift
