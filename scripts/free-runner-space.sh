#!/usr/bin/env bash
# Only disposable Linux CI runners may discard their unused image SDKs.
set -euo pipefail
[[ ${GITHUB_ACTIONS:-} == true && ${RUNNER_OS:-} == Linux ]] || {
    echo 'SDK cleanup requires a disposable GitHub Actions Linux runner.' >&2
    exit 2
}
[[ $# -gt 0 ]] || { echo 'Pass the unused SDK directories to reclaim.' >&2; exit 2; }
# Pinned build/test environments, restored Cargo outputs and a disposable AVD.
required_kib=$((35 * 1024 * 1024))
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
    echo 'At least 35 GiB of free space is required for native CI.' >&2
    exit 1
}
