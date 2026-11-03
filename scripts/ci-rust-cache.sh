#!/usr/bin/env bash
# Preserve repository crates omitted by the dependency cache action.
set -euo pipefail
case ${CARGO_MANIFEST_DIR:-}/ in
    "${GITHUB_WORKSPACE:?CI workspace is required}/"*) exec kache "$@" ;;
    *) exec "$@" ;;
esac
