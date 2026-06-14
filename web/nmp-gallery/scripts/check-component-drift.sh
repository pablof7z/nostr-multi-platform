#!/usr/bin/env bash
# Assert the gallery's vendored web components are byte-identical to the
# canonical registry source. The registry (web/registry/src/vendor/web) is the
# single source of truth; the gallery vendors a copy so it deploys
# self-contained. Relative sibling imports keep the copies byte-identical, so
# any difference is drift and fails the build.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GALLERY_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$GALLERY_DIR/../.." && pwd)"

CANONICAL="$REPO_ROOT/web/registry/src/vendor/web"
VENDORED="$GALLERY_DIR/src/components"

if diff -r "$CANONICAL" "$VENDORED"; then
  echo "[drift] gallery components are byte-identical to the registry canonical source."
else
  echo "[drift] ERROR: gallery vendored components diverge from web/registry/src/vendor/web."
  echo "[drift] Re-sync with: cp -r $CANONICAL/* $VENDORED/"
  exit 1
fi
