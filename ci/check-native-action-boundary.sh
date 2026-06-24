#!/usr/bin/env bash
#
# Native action write-boundary gate.
#
# Migrated action namespaces are owned by the generated action builders. Swift
# and Kotlin production code must call those builders (or a Rust-authored intent
# seam) instead of spelling the namespace literals by hand.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
python3 "${SCRIPT_DIR}/check_native_action_boundary.py" "$@"
