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

extract_public_npm_package_names() {
  awk -F '"' '
    /^\[\[public_npm_packages\]\]/ { in_public = 1; next }
    /^\[\[/ && $0 !~ /^\[\[public_npm_packages\]\]/ { in_public = 0 }
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

public_npm_packages="$(extract_public_npm_package_names)"
if [[ -n "$public_npm_packages" ]]; then
  npm --prefix "$ROOT/web" ci
  while read -r package; do
    [[ -n "$package" ]] || continue
    echo "npm run build --workspace $package"
    npm --prefix "$ROOT/web" run build --workspace "$package"
    echo "npm pack --dry-run --workspace $package"
    npm --prefix "$ROOT/web" pack --workspace "$package" --dry-run --ignore-scripts >/dev/null
  done <<< "$public_npm_packages"
fi

echo "release package dry-run ok"
