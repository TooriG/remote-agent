#!/usr/bin/env bash
# Both public APK workflows use the same signing key and source version.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${AGENT_ANDROID_PREVIEW_KEYSTORE_BASE64:?Configure the existing preview signing key before publishing APKs}"
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
keystore="$RUNNER_TEMP/agent-preview.p12"
trap 'rm -f "$keystore"' ERR
printf '%s' "$AGENT_ANDROID_PREVIEW_KEYSTORE_BASE64" | base64 --decode > "$keystore"
test -s "$keystore"
printf 'AGENT_ANDROID_PREVIEW_KEYSTORE=%s\nAGENT_ANDROID_VERSION_CODE=%s\n' "$keystore" "$version" >> "$GITHUB_ENV"
