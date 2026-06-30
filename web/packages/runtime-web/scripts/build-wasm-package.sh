#!/usr/bin/env bash
set -euo pipefail

PACKAGE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="$(cd "${PACKAGE_DIR}/../../.." && pwd)"
WASM_OUT="${ROOT}/pkg/nmp-browser-runtime"
STAGED="${PACKAGE_DIR}/dist/wasm"

bash "${ROOT}/crates/nmp-browser-runtime/scripts/build-wasm.sh"

rm -rf "${STAGED}"
mkdir -p "${STAGED}"
cp "${WASM_OUT}"/nmp_browser_runtime* "${STAGED}/"

if [[ -d "${WASM_OUT}/snippets" ]]; then
  cp -R "${WASM_OUT}/snippets" "${STAGED}/snippets"
fi
