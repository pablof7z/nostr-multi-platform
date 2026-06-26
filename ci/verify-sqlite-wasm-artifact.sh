#!/usr/bin/env bash
# verify-sqlite-wasm-artifact.sh — supply-chain re-verify gate for the vendored
# SQLite-WASM artifact in `crates/nmp-sqlite-wasm/vendor/sqlite-wasm/`.
#
# The vendored `sqlite3.wasm` + `sqlite3.mjs` are a pre-compiled, public-domain
# artifact downloaded from sqlite.org (see vendor/PROVENANCE.md). Because they
# live outside the Cargo dependency graph, they are also outside the only
# automated supply-chain control (cargo-deny / cargo-audit). This script is the
# substitute control: it recomputes the SHA-256 of each vendored upstream file
# and fails on any mismatch with the pinned manifest (`SHA256SUMS`). A drift
# here means the vendored binary changed without a reviewed provenance update —
# exactly what the gate exists to catch.
#
# Scope: ONLY the two opaque upstream files are pinned. `nmp-sqlite3-shim.mjs`
# is first-party NMP source (its integrity is git history + the file-size and
# doctrine gates), not a vendored binary, so it is intentionally not listed.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VENDOR_DIR="$REPO_ROOT/crates/nmp-sqlite-wasm/vendor/sqlite-wasm"
SUMS_FILE="$VENDOR_DIR/SHA256SUMS"

if [[ ! -f "$SUMS_FILE" ]]; then
  echo "FAIL: missing checksum manifest: $SUMS_FILE" >&2
  exit 1
fi

# Pick whichever SHA-256 checker is present (sha256sum on Linux/CI, shasum on
# macOS). Both accept the `-c` verify mode with the same manifest format.
if command -v sha256sum >/dev/null 2>&1; then
  CHECK=(sha256sum -c --strict)
elif command -v shasum >/dev/null 2>&1; then
  CHECK=(shasum -a 256 -c)
else
  echo "FAIL: neither sha256sum nor shasum is available" >&2
  exit 1
fi

cd "$VENDOR_DIR"
echo "Verifying vendored SQLite-WASM artifact integrity in $VENDOR_DIR"
if "${CHECK[@]}" SHA256SUMS; then
  echo "OK: vendored SQLite-WASM artifact matches the pinned SHA-256 manifest."
else
  echo "FAIL: vendored SQLite-WASM artifact does not match SHA256SUMS." >&2
  echo "      The vendored binary changed without a reviewed provenance update." >&2
  echo "      See crates/nmp-sqlite-wasm/vendor/PROVENANCE.md for the re-vendor procedure." >&2
  exit 1
fi
