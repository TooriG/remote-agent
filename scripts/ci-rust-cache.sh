#!/usr/bin/env bash
# Preserve vendored libraries omitted by the dependency cache action.
set -euo pipefail
case ${CARGO_MANIFEST_DIR:-}/ in
    "${GITHUB_WORKSPACE:?CI workspace is required}/vendor/"*) exec kache "$@" ;;
    *) exec "$@" ;;
esac
