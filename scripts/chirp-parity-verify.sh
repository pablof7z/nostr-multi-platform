#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "== Rust parity checks =="
cargo test -p nmp-wasm
cargo test -p chirp-repl
cargo test -p nmp-core t140_m2_follow_feed
cargo test -p nmp-core contacts_fanout

echo "== Browser wasm package =="
if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "wasm-pack is required to refresh web/chirp/public/nmp-wasm" >&2
  exit 1
fi
wasm-pack build "$ROOT/crates/nmp-wasm" --target web --out-dir "$ROOT/web/chirp/public/nmp-wasm"
rm -f "$ROOT/web/chirp/public/nmp-wasm/.gitignore"

echo "== Web build and unit checks =="
cd "$ROOT/web/chirp"
npm ci
npm run build
npm run test

cat <<'MSG'

Core parity checks passed.

Browser smoke is separate because it needs a running preview server:
  cd web/chirp
  npm run preview -- --host 127.0.0.1 --port 4173

Then, from the repo root:
  scripts/chirp-web-browser-smoke.sh http://127.0.0.1:4173/
MSG
