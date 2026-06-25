#!/usr/bin/env bash
# Build the nmp-app-chirp-web package, then build the Chirp web app.
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
WEB_GALLERY_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$WEB_GALLERY_DIR/../.." && pwd)"
CRATE_DIR="$REPO_ROOT/apps/chirp/crates/nmp-app-chirp-web"
OUT_DIR="$WEB_GALLERY_DIR/public/nmp-wasm"

# $HOME/.cargo/bin may not exist if cargo was installed system-wide (e.g.
# the Vercel build image).  Create it unconditionally and add it to PATH so
# that tools we drop there (wasm-pack) are immediately visible.
mkdir -p "$HOME/.cargo/bin"
export PATH="$HOME/.cargo/bin:$PATH"

# ---------------------------------------------------------------------------
# 0. Ensure clang (required by secp256k1-sys when cross-compiling to wasm32)
# ---------------------------------------------------------------------------
if ! command -v clang &>/dev/null; then
    echo "[build] clang not found — installing via dnf..."
    dnf install -y clang
fi

# ---------------------------------------------------------------------------
# 1. Ensure Rust toolchain
# ---------------------------------------------------------------------------
if ! command -v cargo &>/dev/null; then
    echo "[build] Rust not found — installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path
    # Source the env file if present, then fall back to exporting bin dir.
    # shellcheck disable=SC1091
    if [ -f "$HOME/.cargo/env" ]; then
        source "$HOME/.cargo/env"
    fi
fi

rustup target add wasm32-unknown-unknown

# ---------------------------------------------------------------------------
# 2. Ensure wasm-pack 0.13.1
#    Pinned to match the wasm-bindgen version in apps/chirp/crates/nmp-app-chirp-web/Cargo.toml.
#    Use the pre-built binary when possible (saves ~1-2 min vs cargo-install).
# ---------------------------------------------------------------------------
if ! command -v wasm-pack &>/dev/null; then
    echo "[build] wasm-pack not found — fetching pre-built 0.13.1 binary..."
    WASM_PACK_URL="https://github.com/rustwasm/wasm-pack/releases/download/v0.13.1/wasm-pack-v0.13.1-x86_64-unknown-linux-musl.tar.gz"
    WASM_PACK_TMP=$(mktemp -d)
    if curl -fsSL "$WASM_PACK_URL" | tar -xz -C "$WASM_PACK_TMP" && \
       install -m 0755 "$WASM_PACK_TMP"/*/wasm-pack "$HOME/.cargo/bin/wasm-pack"; then
        echo "[build] wasm-pack 0.13.1 installed from pre-built binary."
        rm -rf "$WASM_PACK_TMP"
    else
        echo "[build] pre-built binary failed — falling back to cargo install..."
        rm -rf "$WASM_PACK_TMP"
        cargo install wasm-pack --version 0.13.1 --locked
    fi
fi

# ---------------------------------------------------------------------------
# 3. Build the wasm package
# ---------------------------------------------------------------------------
echo "[build] Building nmp-app-chirp-web (target: web, out: $OUT_DIR)..."
CC_wasm32_unknown_unknown=clang wasm-pack build \
    --target web \
    "$CRATE_DIR" \
    --out-dir "$OUT_DIR"

# ---------------------------------------------------------------------------
# 4. Build the NMP Gallery web app (TypeScript check + Vite bundle)
# ---------------------------------------------------------------------------
echo "[build] Building NMP Gallery web..."
npm --prefix "$REPO_ROOT/web" install
npm --prefix "$REPO_ROOT/web" run build -w @nmp/gallery-web
