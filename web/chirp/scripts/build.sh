#!/usr/bin/env bash
# Build the nmp-browser-runtime wasm package, then build the Chirp web app.
#
# Used by the Vercel deploy build command (see vercel.json) and available
# locally as an alternative to running the two steps manually.
#
# Rust + wasm-pack are installed if not already present.  On CI both are
# pre-installed by earlier workflow steps so the guards are no-ops.
#
# Required compiler: clang with wasm32 support.
#   secp256k1-sys's build.rs compiles C for wasm32. Linux distro clang usually
#   supports that target; Apple clang does not, so prefer Homebrew LLVM when
#   present.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WEB_CHIRP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$WEB_CHIRP_DIR/../.." && pwd)"
CRATE_SCRIPT="$REPO_ROOT/crates/nmp-browser-runtime/scripts/build-wasm.sh"
PKG_OUT="$REPO_ROOT/pkg/nmp-browser-runtime"
DEST_DIR="$WEB_CHIRP_DIR/public/nmp-browser-runtime"

# $HOME/.cargo/bin may not exist if cargo was installed system-wide.
mkdir -p "$HOME/.cargo/bin"
export PATH="$HOME/.cargo/bin:$PATH"
if [ -d /opt/homebrew/opt/llvm/bin ]; then
    export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
fi
if [ -d /usr/local/opt/llvm/bin ]; then
    export PATH="/usr/local/opt/llvm/bin:$PATH"
fi

# ---------------------------------------------------------------------------
# 0. Ensure clang (required by secp256k1-sys when cross-compiling to wasm32)
# ---------------------------------------------------------------------------
if ! command -v clang &>/dev/null; then
    if command -v apt-get &>/dev/null; then
        echo "[build] clang not found — installing via apt-get..."
        if command -v sudo &>/dev/null; then
            sudo apt-get update
            sudo apt-get install -y clang lld llvm
        else
            apt-get update
            apt-get install -y clang lld llvm
        fi
    elif command -v dnf &>/dev/null; then
        echo "[build] clang not found — installing via dnf..."
        dnf install -y clang lld llvm
    else
        echo "ERROR: clang not found and no supported package manager is available."
        exit 1
    fi
fi

# ---------------------------------------------------------------------------
# 1. Ensure Rust toolchain
# ---------------------------------------------------------------------------
if ! command -v cargo &>/dev/null; then
    echo "[build] Rust not found — installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path
    if [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    fi
fi

rustup target add wasm32-unknown-unknown

# ---------------------------------------------------------------------------
# 2. Ensure wasm-pack 0.13.1
#    Pinned to match the wasm-bindgen version in Cargo.toml.
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
# 3. Build the nmp-browser-runtime wasm package
# ---------------------------------------------------------------------------
echo "[build] Building nmp-browser-runtime (target: web, out: $PKG_OUT)..."
: "${CC_wasm32_unknown_unknown:=clang}"
if [ -z "${AR_wasm32_unknown_unknown:-}" ] && command -v llvm-ar &>/dev/null; then
    export AR_wasm32_unknown_unknown=llvm-ar
fi
export CC_wasm32_unknown_unknown
if [ -n "${AR_wasm32_unknown_unknown:-}" ]; then
    export AR_wasm32_unknown_unknown
fi
bash "$CRATE_SCRIPT"

# ---------------------------------------------------------------------------
# 4. Copy wasm output to the chirp public directory
# ---------------------------------------------------------------------------
echo "[build] Copying wasm output to $DEST_DIR..."
rm -rf "$DEST_DIR"
mkdir -p "$DEST_DIR"
cp -r "$PKG_OUT/." "$DEST_DIR/"

echo "[build] Verifying wasm public artifacts..."
shopt -s nullglob
SQLITE_MJS=("$DEST_DIR"/snippets/nmp-sqlite-wasm-*/vendor/sqlite-wasm/sqlite3.mjs)
SQLITE_WASM=("$DEST_DIR"/snippets/nmp-sqlite-wasm-*/vendor/sqlite-wasm/sqlite3.wasm)
if [ ! -f "$DEST_DIR/nmp_browser_runtime.js" ]; then
    echo "ERROR: missing $DEST_DIR/nmp_browser_runtime.js"
    exit 1
fi
if [ ! -f "$DEST_DIR/nmp_browser_runtime_bg.wasm" ]; then
    echo "ERROR: missing $DEST_DIR/nmp_browser_runtime_bg.wasm"
    exit 1
fi
if [ "${#SQLITE_MJS[@]}" -eq 0 ]; then
    echo "ERROR: missing sqlite3.mjs under $DEST_DIR/snippets/nmp-sqlite-wasm-*/vendor/sqlite-wasm/"
    exit 1
fi
if [ "${#SQLITE_WASM[@]}" -eq 0 ]; then
    echo "ERROR: missing sqlite3.wasm under $DEST_DIR/snippets/nmp-sqlite-wasm-*/vendor/sqlite-wasm/"
    exit 1
fi

# ---------------------------------------------------------------------------
# 5. Build the Chirp web app (TypeScript check + Vite bundle)
# ---------------------------------------------------------------------------
echo "[build] Building Chirp web..."
npm --prefix "$REPO_ROOT/web" install
npm --prefix "$REPO_ROOT/web" run build -w @nmp/chirp-web
