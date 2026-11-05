#!/usr/bin/env bash
# Both release channels use one distribution key and the same source version.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${AGENT_ANDROID_KEYSTORE_BASE64:?Configure the BEX distribution signing key before publishing APKs}"
: "${RUNNER_TEMP:?}"
: "${GITHUB_ENV:?}"
[[ $(git rev-parse --is-shallow-repository) == false ]] || {
    echo 'Android publication requires the full source history.' >&2
    exit 1
}
# Commit counts increase along the unrebasable main history, across workflows.
version=$(git rev-list --count HEAD)
(( version > 0 && version <= 2100000000 )) || {
    echo 'Android version code is outside the supported range.' >&2
    exit 1
}
umask 077
keystore="$RUNNER_TEMP/bex-android.p12"
trap 'rm -f "$keystore"' ERR
printf '%s' "$AGENT_ANDROID_KEYSTORE_BASE64" | base64 --decode > "$keystore"
test -s "$keystore"
printf 'AGENT_ANDROID_KEYSTORE=%s\nAGENT_ANDROID_VERSION_CODE=%s\nAGENT_ANDROID_KEY_ALIAS=%s\n' "$keystore" "$version" "${AGENT_ANDROID_KEY_ALIAS:-bex}" >> "$GITHUB_ENV"
