#!/usr/bin/env bash
# Build the NMP Gallery web app.
#
# The wasm composition root (nmp-browser-runtime) will land in PR #2038.
# Until then, this script builds only the TypeScript/Vite bundle.
#
# Used by the Vercel deploy build command (see vercel.json) and available
# locally as an alternative to running the build manually.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WEB_GALLERY_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$WEB_GALLERY_DIR/../.." && pwd)"

# ---------------------------------------------------------------------------
# Build the NMP Gallery web app (TypeScript check + Vite bundle)
# ---------------------------------------------------------------------------
echo "[build] Building NMP Gallery web..."
npm --prefix "$REPO_ROOT/web" install
npm --prefix "$REPO_ROOT/web" run build -w @nmp/gallery-web

echo "[build] ✓ Gallery build complete (wasm from #2038)"
