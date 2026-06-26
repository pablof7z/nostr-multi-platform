#!/usr/bin/env bash
#
# UniFFI Kotlin binding drift gate (M14-0 / issue #2129).
#
# The checked-in UniFFI Kotlin binding for the Android app-loop lane lives at:
#   apps/chirp/android/app/src/main/java/org/nmp/android/uniffi/nmp_android_ffi.kt
#
# It is generated from:
#   apps/chirp/crates/nmp-chirp-android-ffi/src/uniffi_app_loop.rs
# via:
#   cargo run --features bindgen-cli --bin uniffi-bindgen -- generate \
#     --library <host-dylib> --language kotlin --out-dir <java-src-root>/ \
#     --no-format
#
# This script regenerates the bindings with the host (macOS/Linux) build of the
# cdylib and fails on any file difference, so the Rust proc-macro definitions
# and the Kotlin bindings can never drift apart.
#
# Usage:
#   ci/check-uniffi-kotlin-drift.sh           # check (default)
#   ci/check-uniffi-kotlin-drift.sh --write   # regenerate and overwrite
#
# Requirements:
#   - Rust toolchain (cargo) on PATH
#   - Host compiler target (aarch64-apple-darwin or x86_64-unknown-linux-gnu)
#
# The script intentionally builds for the host target (not Android arm64-v8a)
# because uniffi-bindgen uses dlopen to introspect metadata — it does not need
# the Android binary.

set -euo pipefail

MODE="${1:---check}"
case "${MODE}" in
--check|--write) ;;
*)
    echo "uniffi-kotlin-drift: unknown mode '${MODE}' (--check|--write)" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CRATE_DIR="${REPO_ROOT}/apps/chirp/crates/nmp-chirp-android-ffi"
CHECKED_IN_FILE="${REPO_ROOT}/apps/chirp/android/app/src/main/java/org/nmp/android/uniffi/nmp_android_ffi.kt"
JAVA_SRC_ROOT="${REPO_ROOT}/apps/chirp/android/app/src/main/java"

# ── Build host cdylib ────────────────────────────────────────────────────────

echo "uniffi-kotlin-drift: building host cdylib (nmp-chirp-android-ffi)…"
(cd "${CRATE_DIR}" && cargo build --quiet 2>&1) || {
    echo "uniffi-kotlin-drift: cargo build failed — fix compile errors first." >&2
    exit 1
}

# Detect host library extension (macOS = .dylib, Linux = .so)
if [[ "$(uname)" == "Darwin" ]]; then
    LIB="${CRATE_DIR}/target/debug/libnmp_android_ffi.dylib"
else
    LIB="${CRATE_DIR}/target/debug/libnmp_android_ffi.so"
fi

if [[ ! -f "${LIB}" ]]; then
    echo "uniffi-kotlin-drift: expected host library not found: ${LIB}" >&2
    exit 1
fi

# ── Generate bindings into a temp dir ───────────────────────────────────────

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "uniffi-kotlin-drift: generating Kotlin bindings from ${LIB}…"
(cd "${CRATE_DIR}" && cargo run --quiet --features bindgen-cli --bin uniffi-bindgen -- generate \
    --library "${LIB}" \
    --language kotlin \
    --out-dir "${TMP_DIR}/" \
    --no-format 2>&1) || {
    echo "uniffi-kotlin-drift: uniffi-bindgen generate failed." >&2
    exit 1
}

FRESH_FILE="${TMP_DIR}/org/nmp/android/uniffi/nmp_android_ffi.kt"

if [[ ! -f "${FRESH_FILE}" ]]; then
    echo "uniffi-kotlin-drift: expected generated file not found: ${FRESH_FILE}" >&2
    echo "  (check that uniffi.toml sets package_name = \"org.nmp.android.uniffi\")" >&2
    exit 1
fi

# ── Check or write ───────────────────────────────────────────────────────────

if [[ "${MODE}" == "--write" ]]; then
    mkdir -p "$(dirname "${CHECKED_IN_FILE}")"
    cp "${FRESH_FILE}" "${CHECKED_IN_FILE}"
    echo "uniffi-kotlin-drift: wrote ${CHECKED_IN_FILE}"
    exit 0
fi

# --check mode: diff and report
if ! diff -u "${CHECKED_IN_FILE}" "${FRESH_FILE}"; then
    echo "" >&2
    echo "uniffi-kotlin-drift: checked-in UniFFI Kotlin binding differs from a" >&2
    echo "fresh 'uniffi-bindgen generate' run over libnmp_android_ffi." >&2
    echo "" >&2
    echo "The checked-in file is:" >&2
    echo "  ${CHECKED_IN_FILE}" >&2
    echo "" >&2
    echo "Regenerate with:" >&2
    echo "  bash ci/check-uniffi-kotlin-drift.sh --write" >&2
    echo "" >&2
    echo "If you changed uniffi_app_loop.rs (types, methods, or callback interfaces)," >&2
    echo "regenerate the bindings and commit the updated .kt file in the same PR." >&2
    exit 1
fi

echo "uniffi-kotlin-drift: OK — checked-in binding is in sync with uniffi_app_loop.rs"
