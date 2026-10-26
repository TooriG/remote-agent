#!/usr/bin/env bash
# Exercise UIKit scroll geometry on macOS without a Simulator or Host.
set -euo pipefail
cd "$(dirname "$0")/.."
sdk=$(xcrun --sdk macosx --show-sdk-path)
target=$(cargo metadata --no-deps --format-version 1 | jq -er .target_directory)
output="$target/qa/ios-scroll"
mkdir -p "$output"
xcrun swiftc -sdk "$sdk" -target "$(uname -m)-apple-ios26.0-macabi" \
    -F "$sdk/System/iOSSupport/System/Library/Frameworks" \
    -L "$sdk/System/iOSSupport/usr/lib" \
    apps/mobile/iosApp/Bex/ConversationScrollObserver.swift \
    apps/mobile/iosApp/BexUITests/Fixtures/scroll-tests.swift \
    -o "$output/scroll-tests"
"$output/scroll-tests"
