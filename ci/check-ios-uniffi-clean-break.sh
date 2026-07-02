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
    python3 - "$BANNED_PATTERN" <<'PY'
import os
import re
import sys

pattern = re.compile(sys.argv[1])
roots = ["apps"]
if os.path.isdir("crates/nmp-component-registry/registry/swiftui"):
    roots.append("crates/nmp-component-registry/registry/swiftui")

extensions = (".swift", ".h", ".m", ".mm", ".modulemap", ".pbxproj", ".xcconfig", ".yml", ".yaml")
excluded_parts = (
    "/android/",
    "/desktop/",
    "/tui/",
    "crates/nmp-codegen/tests/fixtures/",
    "target/",
)

found = False
for root in roots:
    if not os.path.isdir(root):
        continue
    for dirpath, dirnames, filenames in os.walk(root):
        rel_dir = dirpath.replace(os.sep, "/")
        if any(part in f"{rel_dir}/" for part in excluded_parts):
            dirnames[:] = []
            continue
        for filename in filenames:
            if not filename.endswith(extensions):
                continue
            path = os.path.join(dirpath, filename).replace(os.sep, "/")
            if any(part in path for part in excluded_parts):
                continue
            try:
                with open(path, "r", encoding="utf-8", errors="ignore") as handle:
                    for line_no, line in enumerate(handle, 1):
                        if pattern.search(line):
                            print(f"{path}:{line_no}:{line.rstrip()}")
                            found = True
            except OSError as exc:
                print(f"{path}: read failed: {exc}", file=sys.stderr)
                sys.exit(2)

sys.exit(0 if found else 1)
PY
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
    if ! REPO_ROOT="$tmpdir" scan_live_tree >/tmp/nmp-ios-clean-break-self-test.out 2>&1; then
        echo "ERROR: self-test failed to catch a deleted nmp-ffi iOS C symbol" >&2
        cat /tmp/nmp-ios-clean-break-self-test.out >&2
        exit 1
    fi
    if ! grep -q 'nmp_app_start' /tmp/nmp-ios-clean-break-self-test.out; then
        echo "ERROR: self-test did not report the expected deleted C symbol" >&2
        cat /tmp/nmp-ios-clean-break-self-test.out >&2
        exit 1
    fi
    if grep -q 'No such file or directory' /tmp/nmp-ios-clean-break-self-test.out; then
        echo "ERROR: self-test scan reported a missing path instead of a clean match" >&2
        cat /tmp/nmp-ios-clean-break-self-test.out >&2
        exit 1
    fi
    echo "OK: iOS clean-break self-test catches deleted nmp-ffi C symbols."
    exit 0
fi

if matches="$(scan_live_tree)"; then
    echo "ERROR: iOS caller still references deleted reusable nmp-ffi headers or C symbols." >&2
    echo "Use app-owned UniFFI generated bindings for migrated framework APIs." >&2
    echo >&2
    echo "$matches" >&2
    exit 1
fi

echo "OK: iOS callers do not import deleted nmp-ffi headers or call migrated nmp-ffi C symbols."
