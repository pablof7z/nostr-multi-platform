#!/usr/bin/env bash
#
# Kotlin flatc codegen-drift gate (issue #1093).
#
# The checked-in Kotlin transport bindings at
#   android/app/src/main/java/nmp/transport/*.kt
# are the flatc output for crates/nmp-core/schema/nmp_update.fbs, generated
# with flatc 25.2.10 (the Android/Kotlin runtime pin — see
# ci/check-flatbuffers-version-pins.sh and android/app/build.gradle.kts).
#
# This script regenerates the Kotlin bindings with the PINNED flatc version and
# fails on any file difference — so the schema and the Kotlin bindings can never
# drift apart again.  The version mismatch is intentional: the Kotlin runtime
# pin (25.2.10) is different from the Rust+Swift pin (25.12.19); see the comment
# at the top of crates/nmp-core/schema/nmp_update.fbs for the rationale.
#
# Usage: ci/check-kotlin-flatc-drift.sh
# Requires: flatc 25.2.10 on PATH.
#
# To regenerate after an intentional schema change:
#   1. Install flatc 25.2.10 (https://github.com/google/flatbuffers/releases/tag/v25.2.10).
#   2. flatc --kotlin -o android/app/src/main/java/ \
#          crates/nmp-core/schema/nmp_update.fbs
#   3. Verify the output with this script.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

EXPECTED_FLATC_VERSION="25.2.10"
SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/nmp_update.fbs"
CHECKED_IN_DIR="${REPO_ROOT}/android/app/src/main/java/nmp/transport"

# ── flatc availability + version guard ──────────────────────────────────────

if ! command -v flatc >/dev/null 2>&1; then
    echo "kotlin-flatc-drift: flatc not found on PATH." >&2
    echo "  Install flatc ${EXPECTED_FLATC_VERSION} from:" >&2
    echo "  https://github.com/google/flatbuffers/releases/tag/v${EXPECTED_FLATC_VERSION}" >&2
    echo "  (Note: the Kotlin pin is ${EXPECTED_FLATC_VERSION}, distinct from the" >&2
    echo "   Rust+Swift pin 25.12.19 — do not use the wrong version.)" >&2
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
    echo "the Rust+Swift pin (25.12.19).  Do not regenerate Kotlin bindings with the" >&2
    echo "Rust/Swift flatc or the runtime guard call will miscompile." >&2
    exit 1
fi

# ── Regenerate into a temp dir and diff ────────────────────────────────────

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

flatc --kotlin -o "${TMP_DIR}" "${SCHEMA}"
GENERATED_DIR="${TMP_DIR}/nmp/transport"

if ! diff -r "${CHECKED_IN_DIR}" "${GENERATED_DIR}"; then
    echo "" >&2
    echo "kotlin-flatc-drift: checked-in Kotlin transport bindings differ from a" >&2
    echo "fresh 'flatc --kotlin' run over crates/nmp-core/schema/nmp_update.fbs." >&2
    echo "Regenerate with:" >&2
    echo "  flatc --kotlin -o android/app/src/main/java/ \\" >&2
    echo "      crates/nmp-core/schema/nmp_update.fbs" >&2
    echo "(requires flatc ${EXPECTED_FLATC_VERSION} — the Kotlin runtime pin)" >&2
    exit 1
fi

echo "kotlin-flatc-drift: OK (flatc ${EXPECTED_FLATC_VERSION}, bindings in sync)"
