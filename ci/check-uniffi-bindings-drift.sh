#!/usr/bin/env bash
# check-uniffi-bindings-drift.sh — Verify checked-in UniFFI bindings match a
# fresh uniffi-bindgen run.
#
# Usage:
#   bash ci/check-uniffi-bindings-drift.sh          # CI: fail on any diff
#   bash ci/check-uniffi-bindings-drift.sh --regen  # regenerate + commit-ready
#
# This script is the canonical regeneration procedure for
# crates/nmp-uniffi/generated/{swift,kotlin}. Regenerate when the nmp-uniffi
# interface changes (new types, new methods, renamed fields).
#
# CI gate: the codegen-drift workflow runs this script on every push/PR so the
# checked-in bindings can never silently drift from the Rust interface.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REGEN=false

for arg in "$@"; do
    case "$arg" in
        --regen) REGEN=true ;;
        *) echo "Unknown argument: $arg" >&2; exit 1 ;;
    esac
done

# ── Step 1: build the dylib ───────────────────────────────────────────────────
echo "Building nmp-uniffi dylib..."
cargo build -p nmp-uniffi 2>&1

DYLIB="${REPO_ROOT}/target/debug/libnmp_uniffi.dylib"
if [[ ! -f "$DYLIB" ]]; then
    # macOS uses .dylib; Linux uses .so
    DYLIB="${REPO_ROOT}/target/debug/libnmp_uniffi.so"
fi
if [[ ! -f "$DYLIB" ]]; then
    echo "ERROR: could not find libnmp_uniffi.dylib or .so" >&2
    exit 1
fi

# ── Step 2: run uniffi-bindgen into a temp dir ───────────────────────────────
TMPDIR_SWIFT=$(mktemp -d)
TMPDIR_KOTLIN=$(mktemp -d)
trap 'rm -rf "$TMPDIR_SWIFT" "$TMPDIR_KOTLIN"' EXIT

echo "Generating Swift bindings..."
cargo run -p nmp-uniffi --features bindgen --bin uniffi-bindgen \
    -- generate --library "$DYLIB" --language swift --out-dir "$TMPDIR_SWIFT"

echo "Generating Kotlin bindings..."
cargo run -p nmp-uniffi --features bindgen --bin uniffi-bindgen \
    -- generate --library "$DYLIB" --language kotlin --out-dir "$TMPDIR_KOTLIN" --no-format

# UniFFI's Swift/Kotlin generators currently emit trailing spaces in several
# type declarations. Normalize generated text here so the canonical drift gate
# and `git diff --check` agree.
find "$TMPDIR_SWIFT" "$TMPDIR_KOTLIN" -type f -print0 \
    | xargs -0 perl -0pi -e 's/[ \t]+$//mg; s/\n+\z/\n/'

# ── Step 3: diff against checked-in bindings ─────────────────────────────────
GENERATED_SWIFT="${REPO_ROOT}/crates/nmp-uniffi/generated/swift"
GENERATED_KOTLIN="${REPO_ROOT}/crates/nmp-uniffi/generated/kotlin"

if [[ "$REGEN" == "true" ]]; then
    echo "Regenerating checked-in bindings..."
    mkdir -p "$GENERATED_SWIFT" "$GENERATED_KOTLIN"
    cp -r "$TMPDIR_SWIFT"/. "$GENERATED_SWIFT/"
    cp -r "$TMPDIR_KOTLIN"/. "$GENERATED_KOTLIN/"
    echo "Done. Stage and commit crates/nmp-uniffi/generated/ to update the drift baseline."
    exit 0
fi

echo "Diffing against checked-in bindings..."
DIFF_OUT=$(diff -r --brief "$GENERATED_SWIFT" "$TMPDIR_SWIFT" 2>&1 || true)
DIFF_OUT+=$(diff -r --brief "$GENERATED_KOTLIN" "$TMPDIR_KOTLIN" 2>&1 || true)

if [[ -n "$DIFF_OUT" ]]; then
    echo ""
    echo "ERROR: UniFFI bindings are out of date. Regenerate with:"
    echo "  bash ci/check-uniffi-bindings-drift.sh --regen"
    echo ""
    echo "Diff:"
    diff -r "$GENERATED_SWIFT" "$TMPDIR_SWIFT" || true
    diff -r "$GENERATED_KOTLIN" "$TMPDIR_KOTLIN" || true
    exit 1
fi

echo "OK: UniFFI bindings are up to date."
