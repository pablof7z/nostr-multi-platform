#!/usr/bin/env bash
#
# Regenerate every checked-in FlatBuffers binding surface covered by CI drift
# gates, using each platform's pinned flatc version:
#   Rust:              25.12.19
#   Web/TypeScript:   25.9.23
#
# Usage:
#   bash ci/regenerate-flatbuffers.sh
#
# The script downloads flatc into ${FLATC_CACHE_DIR} when the exact pin is not
# already cached. Default cache: ${XDG_CACHE_HOME:-$HOME/.cache}/nmp-flatc.
# It never writes tool binaries into the repository.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# #1723 — flatc version pins are single-sourced from ci/flatc-pins.sh.
# shellcheck source=ci/flatc-pins.sh
source "${SCRIPT_DIR}/flatc-pins.sh"

TMP_ROOT="$(mktemp -d)"
trap 'rm -rf "${TMP_ROOT}"' EXIT

if [[ -z "${FLATC_CACHE_DIR:-}" ]]; then
    if [[ -n "${XDG_CACHE_HOME:-}" ]]; then
        FLATC_CACHE_DIR="${XDG_CACHE_HOME}/nmp-flatc"
    elif [[ -n "${HOME:-}" ]]; then
        FLATC_CACHE_DIR="${HOME}/.cache/nmp-flatc"
    else
        FLATC_CACHE_DIR="${TMP_ROOT}/nmp-flatc"
    fi
fi

require_command() {
    local cmd="$1"
    if ! command -v "${cmd}" >/dev/null 2>&1; then
        echo "regenerate-flatbuffers: required command not found: ${cmd}" >&2
        exit 1
    fi
}

flatc_version() {
    local bin="$1"
    "${bin}" --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1
}

flatc_asset_for_host() {
    case "$(uname -s):$(uname -m)" in
    Linux:*) echo "Linux.flatc.binary.clang++-18.zip" ;;
    Darwin:arm64) echo "Mac.flatc.binary.zip" ;;
    Darwin:x86_64) echo "MacIntel.flatc.binary.zip" ;;
    *)
        echo "regenerate-flatbuffers: unsupported host $(uname -s)/$(uname -m)" >&2
        echo "Set FLATC_CACHE_DIR to a directory containing v<version>/flatc binaries if needed." >&2
        exit 1
        ;;
    esac
}

ensure_flatc() {
    local version="$1"
    local tool_dir="${FLATC_CACHE_DIR}/v${version}"
    local flatc_bin="${tool_dir}/flatc"

    if [[ -x "${flatc_bin}" ]] && [[ "$(flatc_version "${flatc_bin}")" == "${version}" ]]; then
        echo "${tool_dir}"
        return
    fi

    require_command curl
    require_command unzip
    require_command install

    local asset
    asset="$(flatc_asset_for_host)"
    local url="https://github.com/google/flatbuffers/releases/download/v${version}/${asset}"
    local zip_path="${TMP_ROOT}/flatc-${version}.zip"
    local unpack_dir="${TMP_ROOT}/flatc-${version}"

    echo "regenerate-flatbuffers: downloading flatc ${version} (${asset})" >&2
    curl -fsSL -o "${zip_path}" "${url}"
    rm -rf "${unpack_dir}"
    mkdir -p "${unpack_dir}" "${tool_dir}"
    unzip -q -o "${zip_path}" -d "${unpack_dir}"

    local unpacked_flatc
    unpacked_flatc="$(find "${unpack_dir}" -type f -name flatc -print -quit)"
    if [[ -z "${unpacked_flatc}" ]]; then
        echo "regenerate-flatbuffers: flatc binary not found in ${asset}" >&2
        exit 1
    fi
    install -m 0755 "${unpacked_flatc}" "${flatc_bin}"

    local actual
    actual="$(flatc_version "${flatc_bin}")"
    if [[ "${actual}" != "${version}" ]]; then
        echo "regenerate-flatbuffers: downloaded flatc ${actual}, expected ${version}" >&2
        exit 1
    fi

    echo "${tool_dir}"
}

run_with_flatc() {
    local version="$1"
    shift

    local tool_dir
    tool_dir="$(ensure_flatc "${version}")"
    echo "regenerate-flatbuffers: $* (flatc ${version})"
    PATH="${tool_dir}:${PATH}" "$@"
}

require_command rustfmt

cd "${REPO_ROOT}"

run_with_flatc "${FLATC_PIN_RUST_SWIFT}" bash ci/check-rust-flatc-drift.sh --write
run_with_flatc "${FLATC_PIN_TS}" bash ci/check-ts-flatc-drift.sh --write

echo "regenerate-flatbuffers: complete"
echo "regenerate-flatbuffers: verify via the codegen-drift workflow or local pinned-flatc drift checks"
