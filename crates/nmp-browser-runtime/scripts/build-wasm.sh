#!/usr/bin/env bash
# Build NmpWasmRuntime for browser targets using wasm-pack.
#
# Usage:
#   ./crates/nmp-browser-runtime/scripts/build-wasm.sh [--dev]
#
# Outputs:
#   pkg/nmp-browser-runtime/  — wasm-pack output (JS + .wasm + TypeScript defs)
#
# The wasm-pack `--target web` flag produces an ES-module bundle suitable for
# direct import in a Web Worker (no bundler required). For Vite/webpack builds
# the caller may pass `--target bundler` instead.
#
# Prerequisites:
#   cargo install wasm-pack   (or install via https://rustwasm.github.io/wasm-pack)
#   wasm32-unknown-unknown target:  rustup target add wasm32-unknown-unknown

set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE_DIR="$(cd "${CRATE_DIR}/../.." && pwd)"
PKG_OUT="${WORKSPACE_DIR}/pkg/nmp-browser-runtime"
SQLITE_WASM_VENDOR="${WORKSPACE_DIR}/crates/nmp-sqlite-wasm/vendor/sqlite-wasm"

# Optional --dev flag for an unoptimised (debug) build.
PROFILE_FLAG="--release"
if [[ "${1:-}" == "--dev" ]]; then
  PROFILE_FLAG="--dev"
fi

echo "==> Building nmp-browser-runtime for wasm32-unknown-unknown …"
echo "    crate  : ${CRATE_DIR}"
echo "    output : ${PKG_OUT}"
echo "    profile: ${PROFILE_FLAG#--}"

# Ensure wasm-pack is on PATH.
if ! command -v wasm-pack &>/dev/null; then
  echo "ERROR: wasm-pack not found. Install with:"
  echo "  cargo install wasm-pack"
  echo "or visit https://rustwasm.github.io/wasm-pack/installer/"
  exit 1
fi

# Ensure the wasm32 target is installed.
if ! rustup target list --installed 2>/dev/null | grep -q "wasm32-unknown-unknown"; then
  echo "==> Adding wasm32-unknown-unknown target …"
  rustup target add wasm32-unknown-unknown
fi

# Build. Start from an empty package directory so stale wasm-bindgen snippet
# hashes from an older build cannot survive into deploy artifacts.
rm -rf "${PKG_OUT}"
wasm-pack build \
  "${CRATE_DIR}" \
  --target web \
  "${PROFILE_FLAG}" \
  --out-dir "${PKG_OUT}" \
  --features wasm

find "${PKG_OUT}/snippets" -type d -path '*/vendor/sqlite-wasm' | sort | while IFS= read -r SNIPPET_DIR; do
  cp "${SQLITE_WASM_VENDOR}/sqlite3.mjs" "${SQLITE_WASM_VENDOR}/sqlite3.wasm" "${SNIPPET_DIR}/"
  echo "==> Staged sqlite3.mjs + sqlite3.wasm into ${SNIPPET_DIR#${PKG_OUT}/}"
done

echo ""
echo "==> Done. Output in ${PKG_OUT}"
echo "    Files:"
ls "${PKG_OUT}" | sed 's/^/    /'
