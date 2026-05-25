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
#   3. apps/chirp/nmp-app-chirp/src/ffi/ (split from ffi.rs in V-09) +
#      crates/nmp-marmot/src/ffi.rs +
#      crates/nmp-marmot/src/identity.rs +
#      crates/nmp-marmot/src/fetch.rs -> libnmp_app_chirp.a
#      (originally relocated from nmp-app-chirp into nmp-marmot in PR #348;
#       returned from apps/marmot/ to crates/nmp-marmot/ in step 12, 2026-05-25)
#
# The Chirp link is the union of libnmp_app_chirp.a + libnmp_marmot.a (when the
# marmot feature is enabled). The marmot C-ABI symbols live in nmp-marmot. The
# chirp glue is enumerated as an explicit file list (not a
# directory scan) ON PURPOSE: the `ffi/tests.rs` suite is reachable only via a
# `#[cfg(test)] mod tests;` declaration and carries NO file-level
# `#![cfg(...test...)]` inner attribute, so a directory scan would mis-include
# it. New non-test FFI files MUST be appended to `FFI_FILE_ROOTS` below.
#
# Doctrine D0 forbids `nmp-core` depending on app/protocol crates, so the
# broker and chirp glue live in their own archives — but every `nmp_app_*`
# symbol they export is still in the Chirp link and still belongs in the
# header. Scanning only nmp-core would false-flag those ~14 symbols as drift.
#
# AUDITOR NOTE — do NOT verify this header against a single archive.
# A `nm -gU libnmp_app_chirp.a` over just the Chirp glue archive WILL report
# header symbols as "missing", because the Chirp link is the UNION of three
# archives. Symbols genuinely absent from `libnmp_app_chirp.a` but present and
# correct in the build include (verified 2026-05-20):
#   - nmp_app_set_storage_path            -> libnmp_core.a
#   - nmp_signer_broker_init              -> libnmp_signer_broker.a
#   - nmp_app_cancel_bunker_handshake     -> libnmp_signer_broker.a
#   - nmp_app_nostrconnect_uri            -> libnmp_signer_broker.a
#   - nmp_broker_free_string              -> libnmp_signer_broker.a
# Each is exported from its own crate's `staticlib` and reaches the Chirp
# binary via that archive's link line. The authoritative drift check is this
# script (source-of-truth = the three FFI roots below), NOT a per-archive `nm`.
# To audit at the binary level, `nm -gU` ALL THREE archives and union the sets.
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
# construction (different prefix) and are not gated by this script. This is a
# DELIBERATE scope decision, not an oversight: those symbols are stable, few,
# and owned by `nmp-signer-broker`; gating them here would couple this script
# to a second prefix family. They are still declared in NmpCore.h (correctly)
# — auditors should not "fix" this script to chase them.
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
    # Step 11 final (PR #472): the C-ABI surface that used to live in
    # `crates/nmp-core/src/ffi/` moved to its own crate, `nmp-ffi`. The
    # symbols, names, signatures and ABI are byte-stable; only the source
    # path moved.
    "${REPO_ROOT}/crates/nmp-ffi/src"
)
FFI_FILE_ROOTS=(
    "${REPO_ROOT}/crates/nmp-signer-broker/src/ffi.rs"
    # Chirp per-app FFI was split into a ffi/ sub-module directory (V-09).
    # Listed explicitly (not a directory scan) so ffi/tests.rs is excluded —
    # it has no file-level #![cfg(test)] and would pass is_test_only_file() as
    # non-test, but it defines zero #[no_mangle] symbols (it's a caller-only
    # test file, same posture as marmot/ffi/tests.rs).
    "${REPO_ROOT}/apps/chirp/nmp-app-chirp/src/ffi/mod.rs"
    "${REPO_ROOT}/apps/chirp/nmp-app-chirp/src/ffi/actions.rs"
    "${REPO_ROOT}/apps/chirp/nmp-app-chirp/src/ffi/handle.rs"
    "${REPO_ROOT}/apps/chirp/nmp-app-chirp/src/ffi/helpers.rs"
    "${REPO_ROOT}/apps/chirp/nmp-app-chirp/src/ffi/register.rs"
    "${REPO_ROOT}/apps/chirp/nmp-app-chirp/src/ffi/snapshot.rs"
    # Marmot C-ABI lives in nmp-marmot (originally relocated from
    # nmp-app-chirp in PR #348; the crate itself returned from apps/marmot/
    # to crates/nmp-marmot/ in step 12, 2026-05-25). Symbols still land in
    # the Chirp link via nmp-marmot's rlib inclusion.
    "${REPO_ROOT}/crates/nmp-marmot/src/ffi.rs"
    "${REPO_ROOT}/crates/nmp-marmot/src/identity.rs"
    "${REPO_ROOT}/crates/nmp-marmot/src/fetch.rs"
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
        # The first rule also covers the single-line form
        # `#[no_mangle] pub extern "C" fn nmp_app_X(...)` so a future style
        # change cannot silently drop a symbol.
        awk '
            /#\[no_mangle\][[:space:]]*pub[[:space:]]+extern[[:space:]]+"C"[[:space:]]+fn[[:space:]]+nmp_app_/ {
                match($0, /nmp_app_[A-Za-z0-9_]+/)
                if (RSTART > 0) print substr($0, RSTART, RLENGTH)
                armed = 0
                next
            }
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
