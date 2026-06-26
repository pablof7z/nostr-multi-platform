#!/usr/bin/env bash
# build.sh — build the dedicated-Worker OPFS durability harness into `web/pkg/`.
#
# Mirrors crates/nmp-sqlite-wasm-conformance/web/build.sh, but the crate under
# test is the FULL browser runtime (nmp-browser-runtime), which pulls
# `secp256k1-sys` through `nmp-core`. Building that to wasm32 needs a C-to-wasm
# toolchain (clang's wasm backend + llvm-ar). Locally, point at Homebrew LLVM:
#
#   CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang \
#   AR_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/llvm-ar \
#   bash ./build.sh
#
# In CI (.github/workflows/browser-runtime-conformance.yml) the toolchain is
# `clang` + `llvm-ar` installed via apt; CC/AR are exported by the workflow.
#
# Steps:
#   1. cargo build the cdylib for wasm32-unknown-unknown.
#   2. wasm-bindgen --target web → browser-loadable ES module glue + snippets.
#   3. Copy the vendored sqlite3.mjs + sqlite3.wasm next to the COPIED shim
#      snippet (the engine cannot load otherwise — see the sqlite conformance
#      build.sh for the full rationale).
#   4. Copy the static Worker entry + host page into pkg/.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(git -C "$HERE" rev-parse --show-toplevel)"
CRATE="nmp-browser-runtime-conformance"
WASM_NAME="nmp_browser_runtime_conformance"
OUT="$HERE/pkg"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
VENDOR="$REPO_ROOT/crates/nmp-sqlite-wasm/vendor/sqlite-wasm"

echo "==> cargo build ($CRATE, wasm32-unknown-unknown)"
echo "    CC_wasm32_unknown_unknown=${CC_wasm32_unknown_unknown:-<unset>}"
echo "    AR_wasm32_unknown_unknown=${AR_wasm32_unknown_unknown:-<unset>}"
cargo build -p "$CRATE" --target wasm32-unknown-unknown

echo "==> wasm-bindgen --target web → $OUT"
rm -rf "$OUT"
wasm-bindgen --target web --no-typescript \
  --out-dir "$OUT" \
  "$TARGET_DIR/wasm32-unknown-unknown/debug/${WASM_NAME}.wasm"

echo "==> stage vendored sqlite engine next to the copied shim snippet"
# wasm-bindgen names the snippet dir snippets/<crate-hash>/vendor/sqlite-wasm/.
SNIPPET_DIR="$(find "$OUT/snippets" -type d -path '*/vendor/sqlite-wasm' | head -n1)"
if [[ -z "$SNIPPET_DIR" ]]; then
  echo "FAIL: could not locate the copied shim snippet dir under $OUT/snippets" >&2
  echo "      (expected .../vendor/sqlite-wasm). wasm-bindgen layout changed?" >&2
  exit 1
fi
cp "$VENDOR/sqlite3.mjs" "$VENDOR/sqlite3.wasm" "$SNIPPET_DIR/"
echo "    staged sqlite3.mjs + sqlite3.wasm into ${SNIPPET_DIR#$OUT/}"

echo "==> copy static Worker entry + host page"
cp "$HERE/worker.js" "$HERE/index.html" "$OUT/"

echo "OK: harness built into $OUT"
