#!/usr/bin/env bash
# Only disposable Linux CI runners may discard their unused image SDKs.
set -euo pipefail
[[ ${GITHUB_ACTIONS:-} == true && ${RUNNER_OS:-} == Linux ]] || {
    echo 'SDK cleanup requires a disposable GitHub Actions Linux runner.' >&2
    exit 2
}
# The initial build and the later emulator allocation have separate budgets.
required_gib=35
if [[ ${1-} == --required-gib ]]; then
    required_gib=${2-}
    [[ $required_gib =~ ^[1-9][0-9]{0,2}$ ]] || {
        echo 'Pass a positive disk budget of at most 999 GiB.' >&2
        exit 2
    }
    shift 2
fi
[[ $# -gt 0 ]] || { echo 'Pass the unused SDK directories to reclaim.' >&2; exit 2; }
required_kib=$((required_gib * 1024 * 1024))
free_kib() { df --output=avail -k / | tail -n 1; }
df -h /
trap 'df -h /' EXIT
available=$(free_kib)
# Reclaim large binaries first; small-file deletion is only needed on tight images.
for mode in large complete; do
    for sdk in "$@"; do
        ((available < required_kib)) || exit 0
        [[ -d $sdk ]] || continue
        if [[ $mode == large ]]; then
            sudo find "$sdk" -type f -size +10M -delete
        else
            sudo rm -rf -- "$sdk"
        fi
        available=$(free_kib)
    done
done
((available >= required_kib)) || {
    printf 'At least %s GiB of free space is required for native CI.\n' "$required_gib" >&2
    exit 1
}
