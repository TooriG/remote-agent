#!/usr/bin/env bash
# Install an unpacked candidate without stopping or replacing a running Host.
set -euo pipefail
source_dir=$(cd "$(dirname "$0")" && pwd)
revision=$(cat "$source_dir/revision")
[[ $revision =~ ^[a-f0-9]{40}$ ]] || { echo 'Invalid release revision.' >&2; exit 1; }
root=${1:-${HOME}/.local/lib/bex/$revision}
bin=${2:-${HOME}/.local/bin}
[[ ! -e $root ]] || { echo 'Choose a fresh installation directory.' >&2; exit 1; }
mkdir -p "$(dirname "$root")"
mkdir "$root"
# A failed relocation must leave a fresh destination available for a retry.
trap 'rm -rf "$root"' ERR
mkdir -p "$bin"
cp -R "$source_dir/." "$root/"
root=$(cd "$root" && pwd)
for executable in host-daemon agent-provider-supervisor agent-cli; do
    "$root/lib/ld-linux-x86-64.so.2" --library-path "$root/lib" "$root/patchelf" \
        --set-interpreter "$root/lib/ld-linux-x86-64.so.2" "$root/$executable"
done
trap - ERR
for executable in host-daemon agent-cli; do
    ln -sfn "$root/$executable" "$bin/$executable"
done
printf 'Installed %s. Existing Host processes keep running until explicitly updated.\n' "$revision"
