#!/usr/bin/env bash
#
# Kotlin flatc codegen-drift gate (issue #1093, extended for #1288).
#
# The checked-in Kotlin bindings cover four groups, all generated with flatc
# 25.2.10 (the Android/Kotlin runtime pin — see ci/check-flatbuffers-version-pins.sh
# and android/app/build.gradle.kts):
#   transport — android/app/src/main/java/nmp/transport/*.kt
#               (flatc output for crates/nmp-core/schema/nmp_update.fbs)
#   kernel    — android/app/src/main/java/nmp/kernel/*.kt   (issue #1288)
#               (flatc output for every `namespace nmp.kernel` root schema in
#                crates/nmp-core/schema/*.fbs — signer_state, action_lifecycle,
#                action_stages, action_results, relay_diagnostics, accounts, …)
#   embed     — android/app/src/main/java/nmp/embed/*.kt
#               (flatc output for crates/nmp-content/schema/embed_sidecar.fbs)
#   nip02     — android/app/src/main/java/nmp/nip02/*.kt
#               (flatc output for crates/nmp-nip02/schema/follow_list.fbs)
#
# This script regenerates the Kotlin bindings with the PINNED flatc version and
# fails on any file difference — so the schemas and the Kotlin bindings can never
# drift apart again.  The version mismatch is intentional: the Kotlin runtime
# pin (25.2.10) is different from the Rust+Swift pin (25.12.19); see the comment
# at the top of crates/nmp-core/schema/nmp_update.fbs for the rationale.
#
# The kernel root list is DERIVED, not hard-coded: it is `grep -l
# 'namespace nmp.kernel' crates/nmp-core/schema/*.fbs`, so a new nmp.kernel
# projection schema is gated automatically the moment it is added.
#
# Usage:
#   ci/check-kotlin-flatc-drift.sh
#   ci/check-kotlin-flatc-drift.sh --write
# Requires: flatc 25.2.10 on PATH.
#
# To regenerate after an intentional schema change:
#   bash ci/regenerate-flatbuffers.sh

set -euo pipefail

MODE="${1:---check}"
case "${MODE}" in
--check|--write) ;;
*)
    echo "kotlin-flatc-drift: unknown mode '${MODE}' (--check|--write)" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# #1723 — flatc version pins are single-sourced from ci/flatc-pins.sh.
# shellcheck source=ci/flatc-pins.sh
source "${SCRIPT_DIR}/flatc-pins.sh"
EXPECTED_FLATC_VERSION="${FLATC_PIN_KOTLIN}"
SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/nmp_update.fbs"
CHECKED_IN_DIR="${REPO_ROOT}/android/app/src/main/java/nmp/transport"
KERNEL_SCHEMA_DIR="${REPO_ROOT}/crates/nmp-core/schema"
KERNEL_CHECKED_IN_DIR="${REPO_ROOT}/android/app/src/main/java/nmp/kernel"

# ── flatc availability + version guard ──────────────────────────────────────

if ! command -v flatc >/dev/null 2>&1; then
    echo "kotlin-flatc-drift: flatc not found on PATH." >&2
    echo "  Install flatc ${EXPECTED_FLATC_VERSION} from:" >&2
    echo "  https://github.com/google/flatbuffers/releases/tag/v${EXPECTED_FLATC_VERSION}" >&2
    echo "  (Note: the Kotlin pin is ${EXPECTED_FLATC_VERSION}, distinct from the" >&2
    echo "   Rust+Swift pin ${FLATC_PIN_RUST_SWIFT} — do not use the wrong version.)" >&2
    exit 1
fi

ACTUAL_FLATC_VERSION="$(flatc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
if [[ "${ACTUAL_FLATC_VERSION}" != "${EXPECTED_FLATC_VERSION}" ]]; then
    echo "kotlin-flatc-drift: flatc ${ACTUAL_FLATC_VERSION} found, but the Kotlin" >&2
    echo "transport bindings are pinned to flatc ${EXPECTED_FLATC_VERSION}" >&2
    echo "(matching the 'flatbuffers-java:${EXPECTED_FLATC_VERSION}' runtime pin in" >&2
    echo " android/app/build.gradle.kts)." >&2
    echo "" >&2
    echo "Install flatc ${EXPECTED_FLATC_VERSION} from:" >&2
    echo "  https://github.com/google/flatbuffers/releases/tag/v${EXPECTED_FLATC_VERSION}" >&2
    echo "" >&2
    echo "NOTE: the Kotlin pin (${EXPECTED_FLATC_VERSION}) is intentionally different from" >&2
    echo "the Rust+Swift pin (${FLATC_PIN_RUST_SWIFT}).  Do not regenerate Kotlin bindings with the" >&2
    echo "Rust/Swift flatc or the runtime guard call will miscompile." >&2
    exit 1
fi

# ── Regenerate into a temp dir and diff ────────────────────────────────────

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

# ── Transport bindings (nmp_update.fbs → nmp/transport/) ────────────────────
flatc --kotlin -o "${TMP_DIR}" "${SCHEMA}"
GENERATED_DIR="${TMP_DIR}/nmp/transport"

if [[ "${MODE}" == "--write" ]]; then
    rm -rf "${CHECKED_IN_DIR}"
    mkdir -p "$(dirname "${CHECKED_IN_DIR}")"
    cp -R "${GENERATED_DIR}" "${CHECKED_IN_DIR}"
elif ! diff -r "${CHECKED_IN_DIR}" "${GENERATED_DIR}"; then
    echo "" >&2
    echo "kotlin-flatc-drift: checked-in Kotlin transport bindings differ from a" >&2
    echo "fresh 'flatc --kotlin' run over crates/nmp-core/schema/nmp_update.fbs." >&2
    echo "Regenerate with:" >&2
    echo "  bash ci/regenerate-flatbuffers.sh" >&2
    exit 1
fi

# ── Kernel bindings (issue #1288) — every `namespace nmp.kernel` root schema
#    in crates/nmp-core/schema/*.fbs → nmp/kernel/. The root list is DERIVED by
#    grepping the namespace, never hard-coded, so a new nmp.kernel projection
#    schema is gated automatically. `-I` resolves cross-schema includes (e.g.
#    resolved_profiles.fbs includes profile_card.fbs).
#
#    The Android shell wires typed decoders for a SUBSET of the nmp.kernel
#    projections (the rest are gated on Rust/Swift but not yet rendered on
#    Android). So the assertion is: every checked-in nmp/kernel/*.kt MUST be
#    byte-identical to its fresh-generated counterpart (catching the silent
#    drift class of #1288), and a checked-in table that vanished from the fresh
#    output is also a failure. flatc emitting EXTRA tables for not-yet-wired
#    projections is not drift — those simply have no checked-in decoder to gate.
KERNEL_SCHEMAS=()
while IFS= read -r schema; do
    KERNEL_SCHEMAS+=("${schema}")
done < <(grep -l 'namespace nmp.kernel' "${KERNEL_SCHEMA_DIR}"/*.fbs | sort)

if [[ "${#KERNEL_SCHEMAS[@]}" -eq 0 ]]; then
    echo "kotlin-flatc-drift: no 'namespace nmp.kernel' schemas found under" >&2
    echo "  ${KERNEL_SCHEMA_DIR} — the derivation grep is broken." >&2
    exit 1
fi

flatc --kotlin -o "${TMP_DIR}" -I "${KERNEL_SCHEMA_DIR}" "${KERNEL_SCHEMAS[@]}"
KERNEL_GENERATED_DIR="${TMP_DIR}/nmp/kernel"

if [[ "${MODE}" == "--write" ]]; then
    kernel_written=0
    kernel_removed=0
    for checked_in in "${KERNEL_CHECKED_IN_DIR}"/*.kt; do
        base="$(basename "${checked_in}")"
        fresh="${KERNEL_GENERATED_DIR}/${base}"
        if [[ -f "${fresh}" ]]; then
            cp "${fresh}" "${checked_in}"
            kernel_written=$((kernel_written + 1))
        else
            rm -f "${checked_in}"
            kernel_removed=$((kernel_removed + 1))
        fi
    done
fi

kernel_drift=0
kernel_checked=0
for checked_in in "${KERNEL_CHECKED_IN_DIR}"/*.kt; do
    base="$(basename "${checked_in}")"
    fresh="${KERNEL_GENERATED_DIR}/${base}"
    if [[ ! -f "${fresh}" ]]; then
        echo "kotlin-flatc-drift: checked-in kernel binding ${base} has no" >&2
        echo "  counterpart in a fresh flatc run over the nmp.kernel schemas —" >&2
        echo "  its source table was renamed or removed from the schema." >&2
        kernel_drift=$((kernel_drift + 1))
        continue
    fi
    if ! diff -u "${checked_in}" "${fresh}"; then
        echo "kotlin-flatc-drift: kernel binding ${base} drifted from a fresh run." >&2
        kernel_drift=$((kernel_drift + 1))
        continue
    fi
    kernel_checked=$((kernel_checked + 1))
done

if [[ "${kernel_drift}" -ne 0 ]]; then
    echo "" >&2
    echo "kotlin-flatc-drift: ${kernel_drift} kernel binding(s) drifted from a fresh" >&2
    echo "'flatc --kotlin' run over the nmp.kernel root schemas. Regenerate with:" >&2
    echo "  bash ci/regenerate-flatbuffers.sh" >&2
    exit 1
fi

# ── Embed sidecar bindings (#1283/#1335 item 2) — nmp.embed namespace ───────
#
# `embed_sidecar.fbs` (crates/nmp-content/schema/) uses `namespace nmp.embed`
# (distinct from nmp.kernel) and has no `include` directives, so it is
# generated independently into `nmp/embed/`.  The drift assertion is identical
# to the kernel check: every checked-in nmp/embed/*.kt must be byte-identical
# to a fresh flatc run.
EMBED_SCHEMA="${REPO_ROOT}/crates/nmp-content/schema/embed_sidecar.fbs"
EMBED_CHECKED_IN_DIR="${REPO_ROOT}/android/app/src/main/java/nmp/embed"

flatc --kotlin -o "${TMP_DIR}" "${EMBED_SCHEMA}"
EMBED_GENERATED_DIR="${TMP_DIR}/nmp/embed"

if [[ "${MODE}" == "--write" ]]; then
    rm -rf "${EMBED_CHECKED_IN_DIR}"
    mkdir -p "$(dirname "${EMBED_CHECKED_IN_DIR}")"
    cp -R "${EMBED_GENERATED_DIR}" "${EMBED_CHECKED_IN_DIR}"
    embed_checked=0
    for checked_in in "${EMBED_CHECKED_IN_DIR}"/*.kt; do
        [[ -f "${checked_in}" ]] || continue
        embed_checked=$((embed_checked + 1))
    done
else
embed_drift=0
embed_checked=0
if [[ -d "${EMBED_CHECKED_IN_DIR}" ]]; then
    for checked_in in "${EMBED_CHECKED_IN_DIR}"/*.kt; do
        [[ -f "${checked_in}" ]] || continue
        base="$(basename "${checked_in}")"
        fresh="${EMBED_GENERATED_DIR}/${base}"
        if [[ ! -f "${fresh}" ]]; then
            echo "kotlin-flatc-drift: checked-in embed binding ${base} has no" >&2
            echo "  counterpart in a fresh flatc run over embed_sidecar.fbs —" >&2
            echo "  its source table was renamed or removed from the schema." >&2
            embed_drift=$((embed_drift + 1))
            continue
        fi
        if ! diff -u "${checked_in}" "${fresh}"; then
            echo "kotlin-flatc-drift: embed binding ${base} drifted from a fresh run." >&2
            embed_drift=$((embed_drift + 1))
            continue
        fi
        embed_checked=$((embed_checked + 1))
    done
fi

if [[ "${embed_drift}" -ne 0 ]]; then
    echo "" >&2
    echo "kotlin-flatc-drift: ${embed_drift} embed binding(s) drifted from a fresh" >&2
    echo "'flatc --kotlin' run over crates/nmp-content/schema/embed_sidecar.fbs." >&2
    echo "Regenerate with:" >&2
    echo "  flatc --kotlin -o android/app/src/main/java/ \\" >&2
    echo "      crates/nmp-content/schema/embed_sidecar.fbs" >&2
    echo "(requires flatc ${EXPECTED_FLATC_VERSION} — the Kotlin runtime pin)" >&2
    exit 1
fi
fi

# ── NIP-02 follow-list binding — nmp.nip02 namespace ───────────────────────
#
# `follow_list.fbs` lives in nmp-nip02 rather than nmp-core because the
# follow-list projection is a reusable NIP-02 building block. Android consumes
# it directly for profile follow-button state, so its generated Kotlin binding
# is drift-gated with the other shell-consumed schemas.
NIP02_SCHEMA="${REPO_ROOT}/crates/nmp-nip02/schema/follow_list.fbs"
NIP02_CHECKED_IN_DIR="${REPO_ROOT}/android/app/src/main/java/nmp/nip02"

flatc --kotlin -o "${TMP_DIR}" "${NIP02_SCHEMA}"
NIP02_GENERATED_DIR="${TMP_DIR}/nmp/nip02"

if [[ "${MODE}" == "--write" ]]; then
    rm -rf "${NIP02_CHECKED_IN_DIR}"
    mkdir -p "$(dirname "${NIP02_CHECKED_IN_DIR}")"
    cp -R "${NIP02_GENERATED_DIR}" "${NIP02_CHECKED_IN_DIR}"
    nip02_checked=0
    for checked_in in "${NIP02_CHECKED_IN_DIR}"/*.kt; do
        [[ -f "${checked_in}" ]] || continue
        nip02_checked=$((nip02_checked + 1))
    done
else
nip02_drift=0
nip02_checked=0
if [[ -d "${NIP02_CHECKED_IN_DIR}" ]]; then
    for checked_in in "${NIP02_CHECKED_IN_DIR}"/*.kt; do
        [[ -f "${checked_in}" ]] || continue
        base="$(basename "${checked_in}")"
        fresh="${NIP02_GENERATED_DIR}/${base}"
        if [[ ! -f "${fresh}" ]]; then
            echo "kotlin-flatc-drift: checked-in NIP-02 binding ${base} has no" >&2
            echo "  counterpart in a fresh flatc run over follow_list.fbs —" >&2
            echo "  its source table was renamed or removed from the schema." >&2
            nip02_drift=$((nip02_drift + 1))
            continue
        fi
        if ! diff -u "${checked_in}" "${fresh}"; then
            echo "kotlin-flatc-drift: NIP-02 binding ${base} drifted from a fresh run." >&2
            nip02_drift=$((nip02_drift + 1))
            continue
        fi
        nip02_checked=$((nip02_checked + 1))
    done
fi

if [[ "${nip02_drift}" -ne 0 ]]; then
    echo "" >&2
    echo "kotlin-flatc-drift: ${nip02_drift} NIP-02 binding(s) drifted from a fresh" >&2
    echo "'flatc --kotlin' run over crates/nmp-nip02/schema/follow_list.fbs." >&2
    echo "Regenerate with:" >&2
    echo "  flatc --kotlin -o android/app/src/main/java/ \\" >&2
    echo "      crates/nmp-nip02/schema/follow_list.fbs" >&2
    echo "(requires flatc ${EXPECTED_FLATC_VERSION} — the Kotlin runtime pin)" >&2
    exit 1
fi
fi

# ── Reference row-delta bindings (ADR-0063 / #1671) — nmp.refs namespace ────
#
# `ref_rowdelta.fbs` (crates/nmp-core/schema/) uses `namespace nmp.refs` and is
# the keyed-projection row-delta payload the Android `KeyedRefCache` decodes.
# Generated independently into `nmp/refs/`; drift assertion identical to embed.
REFS_SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/ref_rowdelta.fbs"
REFS_CHECKED_IN_DIR="${REPO_ROOT}/android/app/src/main/java/nmp/refs"

flatc --kotlin -o "${TMP_DIR}" "${REFS_SCHEMA}"
REFS_GENERATED_DIR="${TMP_DIR}/nmp/refs"

if [[ "${MODE}" == "--write" ]]; then
    rm -rf "${REFS_CHECKED_IN_DIR}"
    mkdir -p "$(dirname "${REFS_CHECKED_IN_DIR}")"
    cp -R "${REFS_GENERATED_DIR}" "${REFS_CHECKED_IN_DIR}"
    refs_checked=0
    for checked_in in "${REFS_CHECKED_IN_DIR}"/*.kt; do
        [[ -f "${checked_in}" ]] || continue
        refs_checked=$((refs_checked + 1))
    done
else
refs_drift=0
refs_checked=0
if [[ -d "${REFS_CHECKED_IN_DIR}" ]]; then
    for checked_in in "${REFS_CHECKED_IN_DIR}"/*.kt; do
        [[ -f "${checked_in}" ]] || continue
        base="$(basename "${checked_in}")"
        fresh="${REFS_GENERATED_DIR}/${base}"
        if [[ ! -f "${fresh}" ]]; then
            echo "kotlin-flatc-drift: checked-in refs binding ${base} has no" >&2
            echo "  counterpart in a fresh flatc run over ref_rowdelta.fbs —" >&2
            echo "  its source table was renamed or removed from the schema." >&2
            refs_drift=$((refs_drift + 1))
            continue
        fi
        if ! diff -u "${checked_in}" "${fresh}"; then
            echo "kotlin-flatc-drift: refs binding ${base} drifted from a fresh run." >&2
            refs_drift=$((refs_drift + 1))
            continue
        fi
        refs_checked=$((refs_checked + 1))
    done
fi

if [[ "${refs_drift}" -ne 0 ]]; then
    echo "" >&2
    echo "kotlin-flatc-drift: ${refs_drift} refs binding(s) drifted from a fresh" >&2
    echo "'flatc --kotlin' run over crates/nmp-core/schema/ref_rowdelta.fbs." >&2
    echo "Regenerate with:" >&2
    echo "  flatc --kotlin -o android/app/src/main/java/ \\" >&2
    echo "      crates/nmp-core/schema/ref_rowdelta.fbs" >&2
    echo "(requires flatc ${EXPECTED_FLATC_VERSION} — the Kotlin runtime pin)" >&2
    exit 1
fi
fi

if [[ "${MODE}" == "--write" ]]; then
    echo "kotlin-flatc-drift: wrote transport + ${kernel_written} kernel (removed ${kernel_removed}) + ${embed_checked} embed + ${nip02_checked} NIP-02 + ${refs_checked} refs bindings (flatc ${EXPECTED_FLATC_VERSION})"
else
    echo "kotlin-flatc-drift: OK (flatc ${EXPECTED_FLATC_VERSION}, transport + ${kernel_checked} kernel + ${embed_checked} embed + ${nip02_checked} NIP-02 + ${refs_checked} refs bindings in sync)"
fi
