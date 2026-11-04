#!/usr/bin/env bash
# Preserve repository crates omitted by the dependency cache action. After an
# exact dependency restore, Cargo recompiles only repository crates and the
# dependents of patched vendored crates, so those dependents are retained too.
set -euo pipefail
case ${CARGO_MANIFEST_DIR:-}/ in
    /) exec "$@" ;;
    "${GITHUB_WORKSPACE:?CI workspace is required}/"*) exec kache "$@" ;;
    *) [[ ${AGENT_CI_DEPENDENCIES_RESTORED:-} == true ]] && exec kache "$@"; exec "$@" ;;
esac
