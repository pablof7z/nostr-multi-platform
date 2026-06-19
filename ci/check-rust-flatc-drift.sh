#!/usr/bin/env bash
#
# Rust flatc codegen-drift gate (PR-B #991/#979).
#
# The checked-in transport bindings at
#   crates/nmp-core/src/transport/generated/nmp_update_generated.rs
# are the flatc output for crates/nmp-core/schema/nmp_update.fbs, formatted
# with `rustfmt --edition 2021`. This script regenerates them with the PINNED
# flatc (must match the `flatbuffers` Rust runtime pin in Cargo.toml, see
# ci/check-flatbuffers-version-pins.sh) and fails on any byte difference —
# so the schema and the Rust bindings can never drift apart again (the gap
# that let a `(deprecated)` field keep stale `payload()`/`add_payload`
# accessors in the checked-in bindings).
#
# Usage:
#   ci/check-rust-flatc-drift.sh
#   ci/check-rust-flatc-drift.sh --write
# Requires: flatc 25.12.19 on PATH, rustfmt (stable toolchain).

set -euo pipefail

MODE="${1:---check}"
case "${MODE}" in
--check|--write) ;;
*)
    echo "rust-flatc-drift: unknown mode '${MODE}' (--check|--write)" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

EXPECTED_FLATC_VERSION="25.12.19"
SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/nmp_update.fbs"
CHECKED_IN="${REPO_ROOT}/crates/nmp-core/src/transport/generated/nmp_update_generated.rs"

if ! command -v flatc >/dev/null 2>&1; then
    echo "rust-flatc-drift: flatc not found on PATH (need ${EXPECTED_FLATC_VERSION})" >&2
    exit 1
fi

ACTUAL_FLATC_VERSION="$(flatc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
if [[ "${ACTUAL_FLATC_VERSION}" != "${EXPECTED_FLATC_VERSION}" ]]; then
    echo "rust-flatc-drift: flatc ${ACTUAL_FLATC_VERSION} found, but the Rust" >&2
    echo "transport bindings are pinned to flatc ${EXPECTED_FLATC_VERSION}" >&2
    echo "(matching the 'flatbuffers = \"${EXPECTED_FLATC_VERSION}\"' runtime pin in Cargo.toml)." >&2
    exit 1
fi

if ! command -v rustfmt >/dev/null 2>&1; then
    echo "rust-flatc-drift: rustfmt not found on PATH" >&2
    exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

flatc --rust -o "${TMP_DIR}" "${SCHEMA}"
rustfmt --edition 2021 "${TMP_DIR}/nmp_update_generated.rs"

if [[ "${MODE}" == "--write" ]]; then
    cp "${TMP_DIR}/nmp_update_generated.rs" "${CHECKED_IN}"
    echo "rust-flatc-drift: wrote ${CHECKED_IN#${REPO_ROOT}/} (flatc ${EXPECTED_FLATC_VERSION})"
    exit 0
fi

if ! diff -u "${CHECKED_IN}" "${TMP_DIR}/nmp_update_generated.rs"; then
    echo "" >&2
    echo "rust-flatc-drift: checked-in Rust transport bindings differ from a" >&2
    echo "fresh 'flatc --rust' run over crates/nmp-core/schema/nmp_update.fbs." >&2
    echo "Regenerate with:" >&2
    echo "  bash ci/regenerate-flatbuffers.sh" >&2
    exit 1
fi

echo "rust-flatc-drift: OK (flatc ${EXPECTED_FLATC_VERSION}, bindings in sync)"
