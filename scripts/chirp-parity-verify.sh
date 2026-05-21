#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "== Rust parity checks =="
cargo test -p nmp-wasm
cargo test -p chirp-repl
cargo test -p nmp-core t140_m2_follow_feed
cargo test -p nmp-core contacts_fanout

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
