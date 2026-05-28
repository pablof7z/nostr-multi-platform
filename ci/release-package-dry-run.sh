#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${1:-$ROOT/release/nmp-release.toml}"

bash "$ROOT/ci/check-release-manifest.sh" "$MANIFEST"

extract_public_crate_names() {
  awk -F '"' '
    /^\[\[public_crates\]\]/ { in_public = 1; next }
    /^\[\[/ && $0 !~ /^\[\[public_crates\]\]/ { in_public = 0 }
    in_public && /^name = / {
      print $2
      in_public = 0
    }
  ' "$MANIFEST"
}

while read -r crate; do
  [[ -n "$crate" ]] || continue
  echo "cargo package --list -p $crate"
  cargo package -p "$crate" --allow-dirty --list >/dev/null
done < <(extract_public_crate_names)

echo "release package dry-run ok"
