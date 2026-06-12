#!/usr/bin/env bash
#
# TypeScript flatc codegen-drift gate (issue #1209).
#
# The checked-in TypeScript transport bindings at
#   web/chirp/src/nmp/generated/nmp/
# are the flatc output for crates/nmp-core/schema/nmp_update.fbs, generated
# with flatc 25.9.23 (the Web/TypeScript runtime pin — see
# ci/check-flatbuffers-version-pins.sh and web/chirp/package.json).
#
# This script regenerates the TypeScript bindings with the PINNED flatc version
# and fails on any file difference — so the schema and the TypeScript bindings
# can never drift apart again.  The version is intentionally different from the
# Rust+Swift pin (25.12.19) and the Kotlin pin (25.2.10); see the comment at
# the top of crates/nmp-core/schema/nmp_update.fbs for the rationale.
#
# NOTE: flatc 25.9.23 and 25.12.19 produce byte-identical TypeScript output for
# this schema, but the version pin is enforced here for consistency with the
# Web/TypeScript runtime pin in web/chirp/package.json.
#
# Usage: ci/check-ts-flatc-drift.sh
# Requires: flatc 25.9.23 on PATH.
#
# To regenerate after an intentional schema change:
#   1. Install flatc 25.9.23 (https://github.com/google/flatbuffers/releases/tag/v25.9.23).
#   2. flatc --ts -o web/chirp/src/nmp/generated/ \
#          crates/nmp-core/schema/nmp_update.fbs
#   3. Verify the output with this script.
#      (requires flatc 25.9.23 — the Web/TypeScript runtime pin)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

EXPECTED_FLATC_VERSION="25.9.23"
SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/nmp_update.fbs"
CHECKED_IN_DIR="${REPO_ROOT}/web/chirp/src/nmp/generated/nmp"

# ── flatc availability + version guard ──────────────────────────────────────

if ! command -v flatc >/dev/null 2>&1; then
    echo "ts-flatc-drift: flatc not found on PATH." >&2
    echo "  Install flatc ${EXPECTED_FLATC_VERSION} from:" >&2
    echo "  https://github.com/google/flatbuffers/releases/tag/v${EXPECTED_FLATC_VERSION}" >&2
    echo "  (Note: the Web/TS pin is ${EXPECTED_FLATC_VERSION}, distinct from the" >&2
    echo "   Rust+Swift pin 25.12.19 and the Kotlin pin 25.2.10.)" >&2
    exit 1
fi

ACTUAL_FLATC_VERSION="$(flatc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
if [[ "${ACTUAL_FLATC_VERSION}" != "${EXPECTED_FLATC_VERSION}" ]]; then
    echo "ts-flatc-drift: flatc ${ACTUAL_FLATC_VERSION} found, but the TypeScript" >&2
    echo "transport bindings are pinned to flatc ${EXPECTED_FLATC_VERSION}" >&2
    echo "(matching the 'flatbuffers: ^${EXPECTED_FLATC_VERSION}' runtime pin in" >&2
    echo " web/chirp/package.json)." >&2
    echo "" >&2
    echo "Install flatc ${EXPECTED_FLATC_VERSION} from:" >&2
    echo "  https://github.com/google/flatbuffers/releases/tag/v${EXPECTED_FLATC_VERSION}" >&2
    echo "" >&2
    echo "NOTE: the Web/TS pin (${EXPECTED_FLATC_VERSION}) is intentionally different from" >&2
    echo "the Rust+Swift pin (25.12.19) and the Kotlin pin (25.2.10)." >&2
    echo "Do not regenerate TypeScript bindings with the Rust/Swift or Kotlin flatc." >&2
    exit 1
fi

# ── Regenerate into a temp dir and diff ────────────────────────────────────

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

flatc --ts -o "${TMP_DIR}" "${SCHEMA}"
GENERATED_DIR="${TMP_DIR}/nmp"

if ! diff -r "${CHECKED_IN_DIR}" "${GENERATED_DIR}"; then
    echo "" >&2
    echo "ts-flatc-drift: checked-in TypeScript transport bindings differ from a" >&2
    echo "fresh 'flatc --ts' run over crates/nmp-core/schema/nmp_update.fbs." >&2
    echo "Regenerate with:" >&2
    echo "  flatc --ts -o web/chirp/src/nmp/generated/ \\" >&2
    echo "      crates/nmp-core/schema/nmp_update.fbs" >&2
    echo "(requires flatc ${EXPECTED_FLATC_VERSION} — the Web/TypeScript runtime pin)" >&2
    exit 1
fi

echo "ts-flatc-drift: OK (flatc ${EXPECTED_FLATC_VERSION}, bindings in sync)"
