#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
case "${1:-apple}" in apple|rust|kotlin|swift) language=${1:-apple} ;; *) echo 'quality expects apple, rust, kotlin, or swift' >&2; exit 2 ;; esac
if (( $# )); then shift; fi
export CARGO_INCREMENTAL=0
# Reject inexpensive failures before compiling apps or starting acceptance.
if [[ $language == apple || $language == swift ]]; then
    swiftformat --lint apps/mobile/iosApp/Bex apps/mobile/iosApp/UITests apps/mobile/iosApp/TaskActivity
    swiftlint lint --strict
fi
if [[ $language == apple || $language == rust ]]; then
    actionlint
    cargo fmt --all --check
    nix build .#agent-peer --no-link
    clippy=(cargo clippy --locked --workspace --all-targets -- --no-deps -D warnings)
    if [[ -n ${AGENT_REMOTE_BUILDER-} ]]; then
        clippy=(scripts/remote-run.sh --native "$AGENT_REMOTE_BUILDER" "${clippy[@]}")
    fi
    "${clippy[@]}"
    integration_targets=(
        --test errors --test iroh --test iroh_host --test browser_bridge
        --test management --test codex_accounts --test claude --test adapter_conformance
        --test crate_boundaries --test build_cleanup --test diagnostics
    )
    just unit-tests "${integration_targets[@]}"
    if [[ $(uname -s) == Darwin ]]; then
        just macos-e2e
    fi
fi
if [[ $language == kotlin ]]; then
    ./gradlew :apps:mobile:ktfmtCheck :apps:mobile:detekt --continue --console=plain
    just android-e2e
fi
if [[ $language == swift ]]; then
    just ios-markdown
fi
if [[ $language == apple || $language == swift ]]; then
    just conversation-ui "$@"
fi
just clean-builds
