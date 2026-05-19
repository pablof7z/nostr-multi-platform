#!/usr/bin/env bash
#
# check-ffi-header-drift.sh — CI gate for C-header / Rust-FFI drift.
#
# `ios/Chirp/Chirp/Bridge/NmpCore.h` is hand-maintained and MUST stay in sync
# with every `#[no_mangle] pub extern "C" fn nmp_app_*` symbol that ships in the
# static archives the Chirp shell links. This script extracts both symbol sets
# and fails (exit 1) on any mismatch in either direction.
#
# SCOPE — the header is a SUPERSET spanning three Rust static archives, so the
# gate scans all three FFI roots (each is read-only — only NmpCore.h, the
# nmp-core ffi/ dir, and ci/ are ever modified by the accompanying change):
#
#   1. crates/nmp-core/src/ffi/            -> libnmp_core.a        (the kernel)
#   2. crates/nmp-signer-broker/src/ffi.rs -> libnmp_signer_broker.a (NIP-46)
#   3. apps/chirp/nmp-app-chirp/src/ffi.rs +
#      apps/chirp/nmp-app-chirp/src/marmot/ffi.rs -> libnmp_app_chirp.a
#
# Doctrine D0 forbids `nmp-core` depending on app/protocol crates, so the
# broker and chirp glue live in their own archives — but every `nmp_app_*`
# symbol they export is still in the Chirp link and still belongs in the
# header. Scanning only nmp-core would false-flag those ~14 symbols as drift.
#
# What counts as a PRODUCTION symbol:
#   - Any `#[no_mangle] pub extern "C" fn nmp_app_*` defined in one of the
#     scanned `.rs` files ...
#   - ... EXCEPT files that are test-only. A file is test-only when its first
#     non-blank, non-comment line is a file-level inner attribute
#     `#![cfg(...)]` whose predicate mentions `test` (covers both `test` and
#     `test-support`). `crates/nmp-core/src/ffi/testing.rs` carries exactly
#     such an attribute.
#
# Why a file-level cfg and not `#[cfg(test)]` per fn: the test-only injectors
# live in a module gated at the `mod testing;` declaration site in `ffi/mod.rs`.
# A `#[cfg]` on a `pub use` re-export does NOT stop `#[no_mangle]` symbol
# emission — the compiler emits the body whenever the defining module compiles.
# So the only reliable "this whole file is test-only" marker is the file-level
# `#![cfg(...)]` inner attribute, which `ffi/testing.rs` declares deliberately.
#
# Non-test cfg gates (e.g. `#[cfg(feature = "wallet")]`, where `wallet` is a
# default feature) do NOT exclude a symbol — those ship in the default build
# and must appear in the header.
#
# The check is restricted to the `nmp_app_*` prefix; broker-only symbols such
# as `nmp_signer_broker_init` / `nmp_broker_free_string` are out of scope by
# construction (different prefix) and are not gated by this script.
#
# Exit codes: 0 = in sync, 1 = drift detected (or usage error).

set -euo pipefail

# ── Resolve paths relative to the repo root (this script lives in ci/). ──────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

HEADER="${REPO_ROOT}/ios/Chirp/Chirp/Bridge/NmpCore.h"

# FFI source roots. Directories are scanned recursively for `*.rs`; explicit
# files are scanned as-is. Each entry is "<path>" — existence is required.
FFI_DIR_ROOTS=(
    "${REPO_ROOT}/crates/nmp-core/src/ffi"
)
FFI_FILE_ROOTS=(
    "${REPO_ROOT}/crates/nmp-signer-broker/src/ffi.rs"
    "${REPO_ROOT}/apps/chirp/nmp-app-chirp/src/ffi.rs"
    "${REPO_ROOT}/apps/chirp/nmp-app-chirp/src/marmot/ffi.rs"
)

if [[ ! -f "${HEADER}" ]]; then
    echo "error: header not found: ${HEADER}" >&2
    exit 1
fi

# ── Build the full list of .rs files to scan. ────────────────────────────────
SCAN_FILES=()
for dir in "${FFI_DIR_ROOTS[@]}"; do
    if [[ ! -d "${dir}" ]]; then
        echo "error: FFI source dir not found: ${dir}" >&2
        exit 1
    fi
    while IFS= read -r f; do
        SCAN_FILES+=("${f}")
    done < <(find "${dir}" -name '*.rs' -type f | sort)
done
for f in "${FFI_FILE_ROOTS[@]}"; do
    if [[ ! -f "${f}" ]]; then
        echo "error: FFI source file not found: ${f}" >&2
        exit 1
    fi
    SCAN_FILES+=("${f}")
done

# ── Is a .rs file test-only? (first non-blank/non-comment line is a ──────────
#    file-level `#![cfg(...test...)]` inner attribute)
is_test_only_file() {
    local file="$1"
    local first_line
    first_line="$(grep -vE '^[[:space:]]*(//.*)?$' "${file}" | head -n 1)"
    [[ "${first_line}" =~ ^#!\[cfg\(.*test.*\)\] ]]
}

# ── Collect production Rust FFI symbols. ─────────────────────────────────────
# `#[no_mangle]` and `pub extern "C" fn` may be on separate lines, so we scan
# for the attribute then look for the next `pub extern "C" fn nmp_app_*`.
RUST_SYMBOLS="$(
    for file in "${SCAN_FILES[@]}"; do
        if is_test_only_file "${file}"; then
            continue
        fi
        # awk: when we see a `#[no_mangle]` line, arm a flag; the next
        # `pub extern "C" fn nmp_app_<name>` line emits <name> and disarms.
        awk '
            /#\[no_mangle\]/ { armed = 1; next }
            armed && /pub[[:space:]]+extern[[:space:]]+"C"[[:space:]]+fn[[:space:]]+nmp_app_/ {
                match($0, /nmp_app_[A-Za-z0-9_]+/)
                if (RSTART > 0) print substr($0, RSTART, RLENGTH)
                armed = 0
                next
            }
            # A non-blank, non-attribute line between the attribute and the fn
            # cancels the pairing (defensive — keeps the matcher honest).
            armed && !/^[[:space:]]*$/ && !/^[[:space:]]*#\[/ { armed = 0 }
        ' "${file}"
    done | sort -u
)"

# ── Collect symbols declared in the C header. ────────────────────────────────
# Match `nmp_app_<name>(` in declaration lines (skip typedef'd callback types,
# which are `(*NmpFooCallback)` — those never look like `nmp_app_name(`).
HEADER_SYMBOLS="$(
    grep -oE 'nmp_app_[A-Za-z0-9_]+[[:space:]]*\(' "${HEADER}" \
        | sed -E 's/[[:space:]]*\(.*//' \
        | sort -u
)"

# ── Diff both directions. ────────────────────────────────────────────────────
RUST_ONLY="$(comm -23 <(printf '%s\n' "${RUST_SYMBOLS}") <(printf '%s\n' "${HEADER_SYMBOLS}"))"
HEADER_ONLY="$(comm -13 <(printf '%s\n' "${RUST_SYMBOLS}") <(printf '%s\n' "${HEADER_SYMBOLS}"))"

DRIFT=0

if [[ -n "${RUST_ONLY}" ]]; then
    DRIFT=1
    echo "FFI DRIFT — nmp_app_* symbols exported from Rust but MISSING in NmpCore.h:" >&2
    while IFS= read -r sym; do
        [[ -z "${sym}" ]] && continue
        loc="$(grep -rl "fn ${sym}\b" "${FFI_DIR_ROOTS[@]}" "${FFI_FILE_ROOTS[@]}" 2>/dev/null | head -n1)"
        echo "  - ${sym}    (defined in ${loc#"${REPO_ROOT}/"})" >&2
    done <<< "${RUST_ONLY}"
fi

if [[ -n "${HEADER_ONLY}" ]]; then
    DRIFT=1
    echo "FFI DRIFT — nmp_app_* symbols declared in NmpCore.h but NOT exported from Rust:" >&2
    while IFS= read -r sym; do
        [[ -n "${sym}" ]] && echo "  - ${sym}" >&2
    done <<< "${HEADER_ONLY}"
fi

if [[ "${DRIFT}" -ne 0 ]]; then
    echo "" >&2
    echo "Fix: add/remove the symbols above so NmpCore.h matches the Rust FFI." >&2
    echo "Test-only injectors (a file-level #![cfg(...test...)], e.g." >&2
    echo "crates/nmp-core/src/ffi/testing.rs) are intentionally excluded — they" >&2
    echo "must NOT appear in the header." >&2
    exit 1
fi

RUST_COUNT="$(printf '%s\n' "${RUST_SYMBOLS}" | grep -c . || true)"
echo "ffi-header-drift: OK — ${RUST_COUNT} production nmp_app_* symbols in sync."
exit 0
