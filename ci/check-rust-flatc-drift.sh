#!/usr/bin/env bash
#
# Rust flatc codegen-drift gate (PR-B #991/#979, hardened for #1931).
#
# Every checked-in Rust FlatBuffers binding named
#   crates/<crate>/src/**/<stem>_generated.rs
# must have an owning schema at
#   crates/<crate>/schema/<stem>.fbs
# This script discovers those pairs from the checked-in generated files,
# regenerates each one with the PINNED flatc (matching the `flatbuffers` Rust
# runtime pin in Cargo.toml; see ci/check-flatbuffers-version-pins.sh), formats
# with `rustfmt --edition 2021`, and fails on any byte difference.
#
# Usage:
#   ci/check-rust-flatc-drift.sh
#   ci/check-rust-flatc-drift.sh --write
#   ci/check-rust-flatc-drift.sh --self-test
# Requires: flatc 25.12.19 on PATH, rustfmt (stable toolchain).

set -euo pipefail

MODE="${1:---check}"
case "${MODE}" in
--check|--write|--self-test) ;;
*)
    echo "rust-flatc-drift: unknown mode '${MODE}' (--check|--write|--self-test)" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

discover_schema_pairs() {
    local root="$1"
    local generated rel stem crate_tail crate_name schema
    local errors=0
    local found=0

    while IFS= read -r generated; do
        found=$((found + 1))
        rel="${generated#${root}/}"
        stem="$(basename "${generated}" _generated.rs)"

        case "${rel}" in
        crates/*/src/*_generated.rs)
            crate_tail="${rel#crates/}"
            crate_name="${crate_tail%%/*}"
            ;;
        *)
            echo "rust-flatc-drift: unsupported checked-in Rust binding path: ${rel}" >&2
            echo "  expected crates/<crate>/src/**/<stem>_generated.rs" >&2
            errors=$((errors + 1))
            continue
            ;;
        esac

        schema="${root}/crates/${crate_name}/schema/${stem}.fbs"
        if [[ ! -f "${schema}" ]]; then
            echo "rust-flatc-drift: checked-in Rust binding has no owning schema: ${rel}" >&2
            echo "  expected ${schema#${root}/}" >&2
            errors=$((errors + 1))
            continue
        fi

        echo "${schema}::${generated}"
    done < <(find "${root}/crates" -type f -path '*/src/*_generated.rs' | sort)

    if ((found == 0)); then
        echo "rust-flatc-drift: no checked-in Rust FlatBuffers bindings found under crates/*/src" >&2
        errors=$((errors + 1))
    fi

    if ((errors > 0)); then
        return 1
    fi
}

self_test() {
    local tmp pairs err
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' RETURN

    mkdir -p "${tmp}/crates/demo/schema" "${tmp}/crates/demo/src/wire/generated"
    cat >"${tmp}/crates/demo/schema/demo.fbs" <<'FBS'
namespace nmp.demo;
table Demo { value:string; }
root_type Demo;
FBS
    : >"${tmp}/crates/demo/src/wire/generated/demo_generated.rs"

    pairs="$(discover_schema_pairs "${tmp}")"
    if [[ "${pairs}" != "${tmp}/crates/demo/schema/demo.fbs::${tmp}/crates/demo/src/wire/generated/demo_generated.rs" ]]; then
        echo "rust-flatc-drift: self-test failed: expected demo schema/generated pair" >&2
        echo "${pairs}" >&2
        return 1
    fi
    echo "rust-flatc-drift: self-test OK — discovers checked-in schema/generated pair"

    : >"${tmp}/crates/demo/src/wire/generated/orphan_generated.rs"
    err="${tmp}/err.txt"
    if discover_schema_pairs "${tmp}" >"${tmp}/pairs.txt" 2>"${err}"; then
        echo "rust-flatc-drift: self-test failed: orphan generated binding passed discovery" >&2
        return 1
    fi
    if ! grep -q 'checked-in Rust binding has no owning schema: crates/demo/src/wire/generated/orphan_generated.rs' "${err}"; then
        echo "rust-flatc-drift: self-test failed: orphan error did not name missing mapping" >&2
        cat "${err}" >&2
        return 1
    fi
    echo "rust-flatc-drift: self-test OK — orphan checked-in binding trips discovery"
}

if [[ "${MODE}" == "--self-test" ]]; then
    self_test
    exit 0
fi

# #1723 — flatc version pins are single-sourced from ci/flatc-pins.sh.
# shellcheck source=ci/flatc-pins.sh
source "${SCRIPT_DIR}/flatc-pins.sh"
EXPECTED_FLATC_VERSION="${FLATC_PIN_RUST_SWIFT}"

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

pairs_file="${TMP_DIR}/schema_pairs.txt"
if ! discover_schema_pairs "${REPO_ROOT}" >"${pairs_file}"; then
    exit 1
fi
SCHEMA_PAIRS=()
while IFS= read -r pair; do
    SCHEMA_PAIRS+=("${pair}")
done <"${pairs_file}"

checked=0
for pair in "${SCHEMA_PAIRS[@]}"; do
    schema="${pair%%::*}"
    checked_in="${pair##*::}"
    basename_rs="$(basename "${checked_in}")"
    pair_tmp="${TMP_DIR}/${basename_rs%.rs}"
    mkdir -p "${pair_tmp}"

    flatc --rust -o "${pair_tmp}" "${schema}"
    if [[ ! -f "${pair_tmp}/${basename_rs}" ]]; then
        echo "rust-flatc-drift: flatc did not emit expected file ${basename_rs}" >&2
        echo "  schema: ${schema#${REPO_ROOT}/}" >&2
        exit 1
    fi
    rustfmt --edition 2021 "${pair_tmp}/${basename_rs}"

    if [[ "${MODE}" == "--write" ]]; then
        cp "${pair_tmp}/${basename_rs}" "${checked_in}"
        echo "rust-flatc-drift: wrote ${checked_in#${REPO_ROOT}/} (flatc ${EXPECTED_FLATC_VERSION})"
        checked=$((checked + 1))
        continue
    fi

    if ! diff -u "${checked_in}" "${pair_tmp}/${basename_rs}"; then
        echo "" >&2
        echo "rust-flatc-drift: checked-in Rust FlatBuffers bindings differ from a" >&2
        echo "fresh 'flatc --rust' run over ${schema#${REPO_ROOT}/}." >&2
        echo "Regenerate with:" >&2
        echo "  bash ci/regenerate-flatbuffers.sh" >&2
        exit 1
    fi
    checked=$((checked + 1))
done

if [[ "${MODE}" == "--write" ]]; then
    echo "rust-flatc-drift: wrote ${checked} Rust binding(s) (flatc ${EXPECTED_FLATC_VERSION})"
    exit 0
fi

echo "rust-flatc-drift: OK (flatc ${EXPECTED_FLATC_VERSION}, ${checked} binding(s) in sync)"
