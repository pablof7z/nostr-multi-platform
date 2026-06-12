#!/usr/bin/env bash
# Build the nmp-wasm package, then build the Chirp web app.
#
# Used by the Vercel deploy build command (see vercel.json) and available
# locally as an alternative to running the two steps manually.
#
# Rust + wasm-pack are installed if not already present.  On CI both are
# pre-installed by earlier workflow steps so the guards are no-ops.
#
# Required env: CC_wasm32_unknown_unknown=clang
#   secp256k1-sys's build.rs compiles C for wasm32; the system clang is the
#   only cc with a wasm32 backend (GCC does not have one).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WEB_CHIRP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$WEB_CHIRP_DIR/../.." && pwd)"
CRATE_DIR="$REPO_ROOT/crates/nmp-wasm"
OUT_DIR="$WEB_CHIRP_DIR/public/nmp-wasm"

# ---------------------------------------------------------------------------
# 1. Ensure Rust toolchain
# ---------------------------------------------------------------------------
if ! command -v cargo &>/dev/null; then
    echo "[build] Rust not found — installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path
    export PATH="$HOME/.cargo/bin:$PATH"
fi

rustup target add wasm32-unknown-unknown

# ---------------------------------------------------------------------------
# 2. Ensure wasm-pack 0.13.1
#    Pinned to match the wasm-bindgen version in crates/nmp-wasm/Cargo.toml.
# ---------------------------------------------------------------------------
if ! command -v wasm-pack &>/dev/null; then
    echo "[build] wasm-pack not found — installing 0.13.1..."
    cargo install wasm-pack --version 0.13.1 --locked
fi

# ---------------------------------------------------------------------------
# 3. Build the wasm package
# ---------------------------------------------------------------------------
echo "[build] Building nmp-wasm (target: web, out: $OUT_DIR)..."
CC_wasm32_unknown_unknown=clang wasm-pack build \
    --target web \
    "$CRATE_DIR" \
    --out-dir "$OUT_DIR"

# ---------------------------------------------------------------------------
# 4. Build the Chirp web app (TypeScript check + Vite bundle)
# ---------------------------------------------------------------------------
echo "[build] Building Chirp web..."
cd "$WEB_CHIRP_DIR"
npm run build
