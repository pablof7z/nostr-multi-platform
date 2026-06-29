#!/usr/bin/env bash
#
# M14-D iOS clean-break ratchet.
#
# After the reusable nmp-ffi framework ABI was deleted, iOS production callers
# must not import its raw headers or call its migrated C symbols. App-owned
# gallery bridge symbols remain allowed because they are scoped to
# apps/nmp-gallery and have no reusable framework ABI counterpart.

set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

BANNED_PATTERN='(#\s*include\s*[<"][^>"]*nmp[-_]ffi|import\s+(nmp_ffi|NmpFfi)|nmp_ffiFFI|NmpFfi|nmp_app_(?!gallery_)(new|free|start|stop|configure|set_|dispatch_|resolve_|release_|add_|remove_|signin_|register_|create_|switch_|load_|open_|close_|search_|retry_|cancel_|ack_|nostrconnect_|init_|deliver_|lifecycle_|debug_|intent_)|nmp_(signer_broker|external_signer|broker|runtime|feed|timeline)_)'

scan_live_tree() {
    cd "$REPO_ROOT"
    rg -n -P "$BANNED_PATTERN" \
        apps crates/nmp-cli/registry/swiftui \
        --glob '*.{swift,h,m,mm,modulemap,pbxproj,xcconfig,yml,yaml}' \
        --glob '!apps/**/android/**' \
        --glob '!apps/**/desktop/**' \
        --glob '!apps/**/tui/**' \
        --glob '!crates/nmp-uniffi/generated/**' \
        --glob '!crates/nmp-codegen/tests/fixtures/**' \
        --glob '!target/**'
}

if [[ "${1:-}" == "--self-test" ]]; then
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT
    mkdir -p "$tmpdir/apps/bad/ios"
    cat > "$tmpdir/apps/bad/ios/Bad.swift" <<'FIXTURE'
func bad(app: UnsafeMutableRawPointer?) {
    nmp_app_start(app, 80, 4)
}
FIXTURE
    if REPO_ROOT="$tmpdir" scan_live_tree >/tmp/nmp-ios-clean-break-self-test.out 2>&1; then
        echo "ERROR: self-test failed to catch a deleted nmp-ffi iOS C symbol" >&2
        exit 1
    fi
    echo "OK: iOS clean-break self-test catches deleted nmp-ffi C symbols."
    exit 0
fi

if matches="$(scan_live_tree)"; then
    echo "ERROR: iOS caller still references deleted reusable nmp-ffi headers or C symbols." >&2
    echo "Use crates/nmp-uniffi generated bindings for migrated framework APIs." >&2
    echo >&2
    echo "$matches" >&2
    exit 1
fi

echo "OK: iOS callers do not import deleted nmp-ffi headers or call migrated nmp-ffi C symbols."
