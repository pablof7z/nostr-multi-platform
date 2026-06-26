#!/usr/bin/env bash
# build.sh — build the dedicated-Worker OPFS conformance harness into `web/pkg/`.
#
# Steps:
#   1. cargo build the cdylib for wasm32-unknown-unknown.
#   2. wasm-bindgen --target web → browser-loadable ES module glue + snippets.
#   3. Copy the vendored sqlite3.mjs + sqlite3.wasm next to the COPIED shim
#      snippet. wasm-bindgen copies the first-party shim glue
#      (nmp-sqlite3-shim.mjs, referenced via `#[wasm_bindgen(module = "/...")]`)
#      into snippets/, but does NOT follow its `import "./sqlite3.mjs"` — it does
#      not parse JS imports. The official sqlite3.mjs in turn fetches
#      sqlite3.wasm relative to its own URL. So both upstream files must sit
#      beside the copied shim or the engine cannot load. This copy is the one
#      bit of glue that makes the vendored artifact reachable from the bundle.
#   4. Copy the static Worker entry + host page into pkg/.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(git -C "$HERE" rev-parse --show-toplevel)"
CRATE="nmp-sqlite-wasm-conformance"
WASM_NAME="nmp_sqlite_wasm_conformance"
OUT="$HERE/pkg"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
VENDOR="$REPO_ROOT/crates/nmp-sqlite-wasm/vendor/sqlite-wasm"

echo "==> cargo build ($CRATE, wasm32-unknown-unknown)"
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
