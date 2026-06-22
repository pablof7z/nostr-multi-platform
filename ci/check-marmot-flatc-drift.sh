#!/usr/bin/env bash
#
# Marmot (NMMS) flatc codegen-drift gate (issue #1240).
#
# The marmot typed-projection schemas
#   crates/nmp-marmot/schema/marmot_snapshot.fbs
#   crates/nmp-marmot/schema/marmot_messages.fbs
# drive checked-in bindings on three platforms:
#   Rust   crates/nmp-marmot/src/wire/generated/marmot_{snapshot,messages}_generated.rs
#   Swift  ios/Chirp/Chirp/Bridge/Generated/Marmot{Snapshot,Messages}.generated.swift
#   Kotlin android/app/src/main/java/nmp/marmot/*.kt
#
# Until #1240 these were NOT gated, so a hand-edit to a schema (or a stale
# checked-in binding) could silently drift from the generated output — exactly
# what PR #1235 (PendingOpRow/LastOpError) risked. This applies the proven
# nmp_update.fbs drift-gate pattern to the marmot schemas.
#
# This is a single script with one mandatory mode argument so the version pins
# stay correct per platform (Rust+Swift use flatc 25.12.19, Kotlin uses
# 25.2.10 — see ci/check-flatbuffers-version-pins.sh):
#
#   ci/check-marmot-flatc-drift.sh rust     # flatc --rust   + rustfmt, 25.12.19
#   ci/check-marmot-flatc-drift.sh swift    # flatc --swift  (rename), 25.12.19
#   ci/check-marmot-flatc-drift.sh kotlin   # flatc --kotlin (dir diff), 25.2.10
#   ci/check-marmot-flatc-drift.sh <mode> --write
#
# Each mode fails on any byte/file difference so the schemas and the checked-in
# bindings can never drift apart again.

set -euo pipefail

MODE="${1:-}"
if [[ -z "${MODE}" ]]; then
    echo "check-marmot-flatc-drift: missing mode argument (rust|swift|kotlin)" >&2
    exit 2
fi
WRITE=0
case "${2:-}" in
"") ;;
--write) WRITE=1 ;;
*)
    echo "check-marmot-flatc-drift: unknown option '${2}' (--write)" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# #1723 — flatc version pins are single-sourced from ci/flatc-pins.sh.
# shellcheck source=ci/flatc-pins.sh
source "${SCRIPT_DIR}/flatc-pins.sh"

SNAPSHOT_SCHEMA="${REPO_ROOT}/crates/nmp-marmot/schema/marmot_snapshot.fbs"
MESSAGES_SCHEMA="${REPO_ROOT}/crates/nmp-marmot/schema/marmot_messages.fbs"

require_flatc_version() {
    local expected="$1"
    if ! command -v flatc >/dev/null 2>&1; then
        echo "marmot-flatc-drift: flatc not found on PATH (need ${expected})" >&2
        exit 1
    fi
    local actual
    actual="$(flatc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
    if [[ "${actual}" != "${expected}" ]]; then
        echo "marmot-flatc-drift: flatc ${actual} found, but this gate is pinned to" >&2
        echo "flatc ${expected} (see ci/check-flatbuffers-version-pins.sh)." >&2
        exit 1
    fi
}

case "${MODE}" in
# ── Rust: flatc --rust 25.12.19 + rustfmt diff ─────────────────────────────
rust)
    require_flatc_version "${FLATC_PIN_RUST_SWIFT}"
    if ! command -v rustfmt >/dev/null 2>&1; then
        echo "marmot-flatc-drift: rustfmt not found on PATH" >&2
        exit 1
    fi

    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT

    drift=0
    written=0
    for stem in marmot_snapshot marmot_messages; do
        flatc --rust -o "${TMP_DIR}" "${REPO_ROOT}/crates/nmp-marmot/schema/${stem}.fbs"
        rustfmt --edition 2021 "${TMP_DIR}/${stem}_generated.rs"
        checked_in="${REPO_ROOT}/crates/nmp-marmot/src/wire/generated/${stem}_generated.rs"
        if [[ "${WRITE}" -eq 1 ]]; then
            cp "${TMP_DIR}/${stem}_generated.rs" "${checked_in}"
            written=$((written + 1))
            continue
        fi
        if ! diff -u "${checked_in}" "${TMP_DIR}/${stem}_generated.rs"; then
            echo "" >&2
            echo "marmot-flatc-drift: ${stem}_generated.rs drifted from a fresh" >&2
            echo "'flatc --rust' + rustfmt run over ${stem}.fbs. Regenerate with:" >&2
            echo "  bash ci/regenerate-flatbuffers.sh" >&2
            drift=$((drift + 1))
        fi
    done
    if [[ "${WRITE}" -eq 1 ]]; then
        echo "marmot-flatc-drift: wrote ${written} Rust bindings (flatc ${FLATC_PIN_RUST_SWIFT})"
        exit 0
    fi
    if [[ "${drift}" -ne 0 ]]; then
        exit 1
    fi
    echo "marmot-flatc-drift: OK rust (flatc ${FLATC_PIN_RUST_SWIFT}, both bindings in sync)"
    ;;

# ── Swift: flatc --swift 25.12.19; flatc emits snake_case, repo renames to
#    PascalCase (marmot_snapshot_generated.swift → MarmotSnapshot.generated.swift).
#    Rename the fresh output before diffing. ─────────────────────────────────
swift)
    require_flatc_version "${FLATC_PIN_RUST_SWIFT}"
    GENERATED_DIR="${REPO_ROOT}/ios/Chirp/Chirp/Bridge/Generated"

    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT

    # "<schema_stem>|<CheckedInPascalCaseFile>"
    MAPPINGS=(
        "marmot_snapshot|MarmotSnapshot.generated.swift"
        "marmot_messages|MarmotMessages.generated.swift"
    )

    drift=0
    written=0
    for entry in "${MAPPINGS[@]}"; do
        IFS='|' read -r stem checked_in_name <<<"${entry}"
        out_subdir="${TMP_DIR}/${stem}"
        mkdir -p "${out_subdir}"
        flatc --swift -o "${out_subdir}" "${REPO_ROOT}/crates/nmp-marmot/schema/${stem}.fbs"

        fresh="${out_subdir}/${stem}_generated.swift"
        renamed="${out_subdir}/${checked_in_name}"
        # Account for the repo's snake_case→PascalCase rename: rename the fresh
        # flatc output to the checked-in name before diffing.
        mv "${fresh}" "${renamed}"

        if [[ "${WRITE}" -eq 1 ]]; then
            cp "${renamed}" "${GENERATED_DIR}/${checked_in_name}"
            written=$((written + 1))
            continue
        fi

        if ! diff -u "${GENERATED_DIR}/${checked_in_name}" "${renamed}"; then
            echo "" >&2
            echo "marmot-flatc-drift: ${checked_in_name} drifted from a fresh" >&2
            echo "'flatc --swift' run over ${stem}.fbs. Regenerate with:" >&2
            echo "  bash ci/regenerate-flatbuffers.sh" >&2
            drift=$((drift + 1))
        fi
    done
    if [[ "${WRITE}" -eq 1 ]]; then
        echo "marmot-flatc-drift: wrote ${written} Swift bindings (flatc ${FLATC_PIN_RUST_SWIFT})"
        exit 0
    fi
    if [[ "${drift}" -ne 0 ]]; then
        exit 1
    fi
    echo "marmot-flatc-drift: OK swift (flatc ${FLATC_PIN_RUST_SWIFT}, both bindings in sync)"
    ;;

# ── Kotlin: flatc --kotlin 25.2.10; dir diff against nmp/marmot. ────────────
kotlin)
    require_flatc_version "${FLATC_PIN_KOTLIN}"
    CHECKED_IN_DIR="${REPO_ROOT}/android/app/src/main/java/nmp/marmot"

    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "${TMP_DIR}"' EXIT

    flatc --kotlin -o "${TMP_DIR}" "${SNAPSHOT_SCHEMA}" "${MESSAGES_SCHEMA}"
    GENERATED_DIR="${TMP_DIR}/nmp/marmot"

    if [[ "${WRITE}" -eq 1 ]]; then
        rm -rf "${CHECKED_IN_DIR}"
        mkdir -p "$(dirname "${CHECKED_IN_DIR}")"
        cp -R "${GENERATED_DIR}" "${CHECKED_IN_DIR}"
        echo "marmot-flatc-drift: wrote Kotlin bindings (flatc ${FLATC_PIN_KOTLIN})"
        exit 0
    fi

    if ! diff -r "${CHECKED_IN_DIR}" "${GENERATED_DIR}"; then
        echo "" >&2
        echo "marmot-flatc-drift: checked-in Kotlin marmot bindings differ from a" >&2
        echo "fresh 'flatc --kotlin' run over the marmot schemas. Regenerate with:" >&2
        echo "  bash ci/regenerate-flatbuffers.sh" >&2
        exit 1
    fi
    echo "marmot-flatc-drift: OK kotlin (flatc ${FLATC_PIN_KOTLIN}, bindings in sync)"
    ;;

*)
    echo "check-marmot-flatc-drift: unknown mode '${MODE}' (rust|swift|kotlin)" >&2
    exit 2
    ;;
esac
