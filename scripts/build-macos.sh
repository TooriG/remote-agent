#!/usr/bin/env bash
# Signing uses the installed Apple toolchain.
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
    [[ $(uname -m) == arm64 ]] || { echo 'The GPUI Mac bundle requires Apple Silicon.' >&2; exit 2; }
    packages+=(--package agent-desktop)
fi
identity=${AGENT_CODE_SIGN_IDENTITY:-}
if [[ -z $identity ]]; then
    identities=$(/usr/bin/security find-identity -v -p codesigning | awk '/"Apple Development:|"Developer ID Application:/ {print $2}')
    [[ -n $identities && $identities != *$'\n'* ]] || {
        echo 'Set AGENT_CODE_SIGN_IDENTITY to one Apple Development or Developer ID Application certificate.' >&2
        exit 1
    }
    identity=$identities
fi
[[ $identity != - ]] || { echo 'Certificate signing is required for Mac builds.' >&2; exit 1; }
output="$(cargo metadata --no-deps --format-version 1 | jq -er .target_directory)/$directory"
export AGENT_BUILD_REVISION
AGENT_BUILD_REVISION=$(git rev-parse HEAD)
if ! git diff --quiet HEAD --; then AGENT_BUILD_REVISION+=-dirty; fi
sign() {
    local options=(--force --sign "$identity")
    if [[ ${AGENT_DISTRIBUTION_SIGNING:-0} == 1 ]]; then
        options+=(--options runtime --timestamp)
    else
        options+=(--timestamp=none)
    fi
    if [[ -n ${AGENT_CODE_SIGN_KEYCHAIN:-} ]]; then options+=(--keychain "$AGENT_CODE_SIGN_KEYCHAIN"); fi
    /usr/bin/codesign "${options[@]}" "$@"
}
verify() { /usr/bin/codesign --verify --deep --strict "$1"; }
cargo build --locked --profile "$profile" "${packages[@]}"
if [[ $product == host ]]; then
    node scripts/install-claude-sdk.mjs "$output"
    sign --identifier dev.remoteagent.provider-supervisor "$output/agent-provider-supervisor"
    verify "$output/agent-provider-supervisor"
    sign --identifier dev.remoteagent.host "$output/host-daemon"
    verify "$output/host-daemon"
    echo "$output/host-daemon"
    exit
fi
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
node_bundle="$bundle/Contents/Helpers/Bex Node.app"
node="$node_bundle/Contents/MacOS/node"
mkdir -p "$executables" "$resources" "${node%/*}"
cp apps/desktop/assets/icon.icns "$resources/bex.icns"
cp crates/host-daemon/src/providers/claude/sdk/SDK-LICENSE.md "$resources/Claude-Agent-SDK-LICENSE.md"
cp "$output/agent-desktop" "$executables/bex"
cp "$output/host-daemon" "$executables/host-daemon"
cp crates/host-daemon/src/providers/claude/sdk/bridge.bundle.mjs "$resources/agent-claude-sdk.mjs"
cp "$output/agent-provider-supervisor" "$executables/agent-provider-supervisor"
cp "${AGENT_NODE_RUNTIME:?Use the pinned Nix development environment}/bin/node" "$node"
cp apps/desktop/macos/Node.Info.plist "$node_bundle/Contents/Info.plist"
cp "$AGENT_NODE_RUNTIME/share/doc/node/LICENSE" "$resources/Node-LICENSE.txt"
cp apps/desktop/macos/Info.plist "$bundle/Contents/Info.plist"
if [[ -n ${AGENT_BUILD_NUMBER:-} ]]; then
    [[ $AGENT_BUILD_NUMBER =~ ^[1-9][0-9]*$ ]] || { echo 'Invalid Mac build number.' >&2; exit 1; }
    /usr/bin/plutil -replace CFBundleVersion -string "$AGENT_BUILD_NUMBER" "$bundle/Contents/Info.plist"
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
sign "$executables/bex"
sign --identifier dev.remoteagent.provider-supervisor "$executables/agent-provider-supervisor"
sign --identifier dev.remoteagent.host "$executables/host-daemon"
sign --options runtime --entitlements apps/desktop/macos/Node.entitlements "$node_bundle"
verify "$node_bundle"
sign "$bundle"
verify "$bundle"
# Exercise the signed runtime without an installed Node or Nix on PATH.
env -i PATH=/usr/bin:/bin "$node" "$resources/agent-claude-sdk.mjs" </dev/null
if [[ -e $destination ]]; then mv "$destination" "$staging/previous.app"; fi
mv "$bundle" "$destination"
echo "$destination"
