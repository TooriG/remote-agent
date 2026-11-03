#!/usr/bin/env bash
set -euo pipefail
[[ $# -ge 2 ]] || { echo "usage: $0 archive-path derived-data-path [xcodebuild arguments...]" >&2; exit 2; }
archive=$1
derived_data=$2
shift 2
cd "$(dirname "$0")/.."
scripts/build-agent-ios.sh device
target=$(cargo metadata --no-deps --format-version 1 | jq -er .target_directory)
exec xcodebuild -project apps/mobile/iosApp/Bex.xcodeproj -scheme Bex \
    -configuration Release -destination 'generic/platform=iOS' \
    -archivePath "$archive" -derivedDataPath "$derived_data" \
    -onlyUsePackageVersionsFromResolvedFile -skipPackagePluginValidation \
    ARCHS=arm64 COMPILER_INDEX_STORE_ENABLE=NO COMPILATION_CACHE_ENABLE_CACHING=YES \
    -allowProvisioningUpdates "AGENT_CARGO_TARGET_DIR=$target" "$@" archive
