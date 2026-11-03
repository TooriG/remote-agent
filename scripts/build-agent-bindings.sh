#!/usr/bin/env bash
# Run inside nix develop. Generated sources stay outside the source tree.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $# -le 1 ]] || { echo "usage: $0 [library]" >&2; exit 2; }
if [[ $# == 1 ]]; then
    library=$1
else
    target=$(cargo metadata --no-deps --format-version 1 | jq -er .target_directory)
    # Core's rlib already contains UniFFI metadata; generating sources needs no FFI link.
    library="$target/debug/libagent_core.rlib"
    cargo build --locked -p agent-core --features bindings --lib
fi
# Preserve timestamps for unchanged bindings, but replace the complete directory
# after a schema change so retired generated files cannot survive.
mkdir -p target
generated=$(mktemp -d target/agent-bindings.XXXXXX)
trap 'rm -rf "$generated"' EXIT
cargo run --locked -p agent-bindgen -- "$library" crates/agent-core/uniffi.toml "$generated"
if [[ ! -d target/agent-bindings ]] || ! diff -qr target/agent-bindings "$generated" >/dev/null; then
    rm -rf target/agent-bindings
    mv "$generated" target/agent-bindings
fi
