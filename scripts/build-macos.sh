#!/usr/bin/env bash
# Signing uses the installed Apple toolchain. AGENT_CODE_SIGN_IDENTITY=- signs
# ad hoc, without the hardened runtime, for checking a bundle on a machine
# without the BEX Developer ID; such a bundle cannot be distributed.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -s) == Darwin ]] || { echo 'Mac builds require macOS' >&2; exit 2; }
case "${1:-desktop}" in
    host|desktop) product=${1:-desktop} ;;
    *) echo "usage: $0 [host|desktop] [dev|release]" >&2; exit 2 ;;
esac
profile=${2:-release}
case "$profile" in
    dev) directory=debug ;;
    release) directory=release ;;
    *) echo "usage: $0 [host|desktop] [dev|release]" >&2; exit 2 ;;
esac
packages=(--package host-daemon --package codex-app-server --package agent-process)
if [[ $product == desktop ]]; then
    [[ $(uname -m) == arm64 ]] || { echo 'The Mac bundle requires Apple Silicon.' >&2; exit 2; }
    packages+=(--package agent-nsh)
fi
if [[ ${AGENT_CODE_SIGN_IDENTITY:-} == - ]]; then
    identity=-
else
    keychains=()
    if [[ -n ${AGENT_CODE_SIGN_KEYCHAIN:-} ]]; then keychains+=("$AGENT_CODE_SIGN_KEYCHAIN"); fi
    identity=$(/usr/bin/security find-identity -v -p codesigning "${keychains[@]}" |
        node scripts/apple-signing.ts identity "${AGENT_CODE_SIGN_IDENTITY:-}")
fi
output="$(cargo metadata --no-deps --format-version 1 | jq -er .target_directory)/$directory"
export AGENT_BUILD_REVISION
AGENT_BUILD_REVISION=$(git rev-parse HEAD)
if ! git diff --quiet HEAD --; then AGENT_BUILD_REVISION+=-dirty; fi
sign() {
    local options=(--force --sign "$identity")
    # Library validation admits the app's framework only under one Team ID.
    if [[ $identity != - ]]; then options+=(--options runtime); fi
    if [[ ${AGENT_DISTRIBUTION_SIGNING:-0} == 1 ]]; then
        options+=(--timestamp)
    else
        options+=(--timestamp=none)
    fi
    if [[ -n ${AGENT_CODE_SIGN_KEYCHAIN:-} ]]; then options+=(--keychain "$AGENT_CODE_SIGN_KEYCHAIN"); fi
    /usr/bin/codesign "${options[@]}" "$@"
}
verify() { /usr/bin/codesign --verify --deep --strict "$1"; }
cargo build --locked --profile "$profile" "${packages[@]}"
if [[ $product == host ]]; then
    node scripts/build-claude-sdk.ts "$output"
    sign --identifier app.bex.provider-supervisor "$output/agent-provider-supervisor"
    verify "$output/agent-provider-supervisor"
    sign --identifier app.bex.host "$output/host-daemon"
    verify "$output/host-daemon"
    echo "$output/host-daemon"
    exit
fi
CARGO_TARGET_DIR="${output%/*}" bash apps/nsh/build-native.sh macos "$directory"
staging=$(mktemp -d "$output/.agent-build.XXXXXX")
destination="$output/bex.app"
cleanup() {
    result=$?
    if [[ -d $staging/previous.app && ! -e $destination ]]; then
        if ! mv "$staging/previous.app" "$destination"; then
            echo "Previous bundle retained in $staging/previous.app" >&2
            exit 1
        fi
    fi
    rm -rf "$staging"
    exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
bundle="$staging/bex.app"
executables="$bundle/Contents/MacOS"
resources="$bundle/Contents/Resources"
frameworks="$bundle/Contents/Frameworks"
framework="$frameworks/BexNshNative.framework"
node_bundle="$bundle/Contents/Helpers/Bex Node.app"
node="$node_bundle/Contents/MacOS/node"
mkdir -p "$executables" "$resources" "$frameworks" "${node%/*}"
node scripts/build-claude-sdk.ts "$resources"
cp "$output/agent-nsh" "$executables/bex"
cp "$output/host-daemon" "$executables/host-daemon"
cp "$output/agent-provider-supervisor" "$executables/agent-provider-supervisor"
# The app loads its Swift views from Contents/Frameworks by path at runtime.
cp -R "${output%/*}/nsh-native/$directory/macos/BexNshNative.framework" "$framework"
cp "${AGENT_NODE_RUNTIME:?Use the pinned Nix development environment}/bin/node" "$node"
cp apps/nsh/macos/Node.Info.plist "$node_bundle/Contents/Info.plist"
cp "$AGENT_NODE_RUNTIME/share/doc/node/LICENSE" "$resources/Node-LICENSE.txt"
cp apps/nsh/macos/Info.plist "$bundle/Contents/Info.plist"
cp apps/nsh/assets/icon.icns "$resources/bex.icns"
# Provider marks and other named images the views show.
xcrun actool apps/mobile/iosApp/Bex/Assets.xcassets --compile "$resources" \
    --platform macosx --minimum-deployment-target 26.0 > /dev/null
/usr/bin/plutil -insert BEXBuildRevision -string "$AGENT_BUILD_REVISION" "$bundle/Contents/Info.plist"
if [[ -n ${AGENT_BUILD_NUMBER:-} ]]; then
    [[ $AGENT_BUILD_NUMBER =~ ^[1-9][0-9]*$ ]] || { echo 'Invalid Mac build number.' >&2; exit 1; }
    /usr/bin/plutil -replace CFBundleVersion -string "$AGENT_BUILD_NUMBER" "$bundle/Contents/Info.plist"
fi
if [[ -n ${AGENT_MARKETING_VERSION:-} ]]; then
    /usr/bin/plutil -replace CFBundleShortVersionString -string "$AGENT_MARKETING_VERSION" "$bundle/Contents/Info.plist"
fi
# Use macOS's libiconv so the app also runs on Macs without Nix.
for executable in "$executables/bex" "$executables/host-daemon" "$executables/agent-provider-supervisor"; do
    while IFS= read -r library; do
        [[ $library == /nix/store/* ]] || continue
        [[ ${library##*/} == libiconv.2.dylib ]] || {
            echo "Unsupported Nix library dependency: $library" >&2; exit 1
        }
        /usr/bin/install_name_tool -change "$library" /usr/lib/libiconv.2.dylib "$executable"
    done < <(/usr/bin/otool -L "$executable" | awk 'NR > 1 {print $1}')
done
sign --identifier app.bex.provider-supervisor "$executables/agent-provider-supervisor"
sign --identifier app.bex.host "$executables/host-daemon"
sign "$framework"
sign --entitlements apps/nsh/macos/Node.entitlements "$node_bundle"
verify "$node_bundle"
sign --entitlements apps/nsh/macos/Desktop.entitlements "$bundle"
verify "$bundle"
if [[ $identity != - ]]; then
    /usr/bin/codesign -d --verbose=2 "$bundle" 2>&1 | grep 'flags=.*runtime' >/dev/null
fi
/usr/bin/codesign -d --entitlements - --xml "$bundle" |
    /usr/bin/plutil -convert json -o - - |
    jq -e '."com.apple.security.device.audio-input" == true' >/dev/null
# Exercise the signed runtime without an installed Node or Nix on PATH.
env -i PATH=/usr/bin:/bin "$node" "$resources/agent-claude-sdk.mjs" </dev/null
# The updater identifies a staged bundle by this answer before replacing the app.
[[ $(env -i PATH=/usr/bin:/bin "$executables/bex" --bex-build-number </dev/null) == "${AGENT_BUILD_NUMBER:-0}" ]] || {
    echo 'The app did not report its build number.' >&2; exit 1
}
if [[ -e $destination ]]; then mv "$destination" "$staging/previous.app"; fi
mv "$bundle" "$destination"
echo "$destination"
