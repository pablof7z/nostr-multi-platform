#!/usr/bin/env bash
#
# C-header / Rust FFI drift gate.
#
# The implementation lives in Python because this check now normalizes both
# Rust extern signatures and Objective-C bridging-header prototypes. Keep this
# shell entrypoint stable for CI and local muscle memory:
#
#   bash ci/check-ffi-header-drift.sh
#   bash ci/check-ffi-header-drift.sh --self-test

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
python3 "${SCRIPT_DIR}/check_ffi_header_drift.py" "$@"
