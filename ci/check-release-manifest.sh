#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${1:-$ROOT/release/nmp-release.toml}"

if [[ ! -f "$MANIFEST" ]]; then
  echo "missing release manifest: $MANIFEST" >&2
  exit 1
fi

if ! grep -q '^schema_version = 1$' "$MANIFEST"; then
  echo "release manifest must declare schema_version = 1" >&2
  exit 1
fi

if ! grep -q '^version_source = "workspace.package.version"$' "$MANIFEST"; then
  echo "release manifest must use workspace.package.version as version source" >&2
  exit 1
fi

extract_public_crates() {
  awk -F '"' '
    /^\[\[public_crates\]\]/ { in_public = 1; name = ""; path = ""; next }
    /^\[\[/ && $0 !~ /^\[\[public_crates\]\]/ { in_public = 0 }
    in_public && /^name = / { name = $2 }
    in_public && /^path = / { path = $2 }
    in_public && name != "" && path != "" {
      print name "|" path
      in_public = 0
    }
  ' "$MANIFEST"
}

extract_private_packages() {
  awk -F '"' '
    /^\[\[private_packages\]\]/ { in_private = 1; name = ""; path = ""; next }
    /^\[\[/ && $0 !~ /^\[\[private_packages\]\]/ { in_private = 0 }
    in_private && /^name = / { name = $2 }
    in_private && /^path = / { path = $2 }
    in_private && name != "" && path != "" {
      print name "|" path
      in_private = 0
    }
  ' "$MANIFEST"
}

workspace_packages() {
  cargo metadata --format-version 1 --no-deps |
    jq -r --arg root "$ROOT/" '
      .workspace_members[] as $id
      | .packages[]
      | select(.id == $id)
      | [.name, (.manifest_path | sub("^" + $root; "") | sub("/Cargo.toml$"; ""))]
      | @tsv
    '
}

count=0
classified="$(mktemp)"
workspace="$(mktemp)"
trap 'rm -f "$classified" "$workspace"' EXIT

while IFS='|' read -r name relpath; do
  [[ -n "$name" ]] || continue
  count=$((count + 1))
  printf '%s\t%s\n' "$name" "$relpath" >> "$classified"
  cargo_toml="$ROOT/$relpath/Cargo.toml"
  if [[ ! -f "$cargo_toml" ]]; then
    echo "public crate $name points at missing manifest: $relpath/Cargo.toml" >&2
    exit 1
  fi
  if ! grep -q "^name = \"$name\"$" "$cargo_toml"; then
    echo "public crate $name path $relpath has a different package name" >&2
    exit 1
  fi
  if ! grep -Eq '^version(\.workspace = true| = \{ workspace = true \})$' "$cargo_toml"; then
    echo "public crate $name must inherit version.workspace = true" >&2
    exit 1
  fi
  if ! grep -Eq '^edition(\.workspace = true| = \{ workspace = true \})$' "$cargo_toml"; then
    echo "public crate $name must inherit edition.workspace = true" >&2
    exit 1
  fi
  if ! grep -Eq '^license(\.workspace = true| = \{ workspace = true \})$' "$cargo_toml"; then
    echo "public crate $name must inherit license.workspace = true" >&2
    exit 1
  fi
  if ! grep -Eq '^repository(\.workspace = true| = \{ workspace = true \})$' "$cargo_toml"; then
    echo "public crate $name must inherit repository.workspace = true" >&2
    exit 1
  fi
  if ! grep -q '^description = ' "$cargo_toml"; then
    echo "public crate $name must declare a crates.io description" >&2
    exit 1
  fi
  if grep -q '^publish = false$' "$cargo_toml"; then
    echo "public crate $name is marked publish = false" >&2
    exit 1
  fi
done < <(extract_public_crates)

while IFS='|' read -r name relpath; do
  [[ -n "$name" ]] || continue
  printf '%s\t%s\n' "$name" "$relpath" >> "$classified"
  cargo_toml="$ROOT/$relpath/Cargo.toml"
  if [[ ! -f "$cargo_toml" ]]; then
    echo "private package $name points at missing manifest: $relpath/Cargo.toml" >&2
    exit 1
  fi
  if ! grep -q "^name = \"$name\"$" "$cargo_toml"; then
    echo "private package $name path $relpath has a different package name" >&2
    exit 1
  fi
done < <(extract_private_packages)

if [[ "$count" -eq 0 ]]; then
  echo "release manifest declares no public crates" >&2
  exit 1
fi

duplicates="$(sort "$classified" | uniq -d)"
if [[ -n "$duplicates" ]]; then
  echo "packages classified more than once:" >&2
  echo "$duplicates" >&2
  exit 1
fi

workspace_packages | sort > "$workspace"
sort "$classified" > "$classified.sorted"
mv "$classified.sorted" "$classified"

if ! missing="$(comm -23 "$workspace" "$classified")" || [[ -n "$missing" ]]; then
  echo "workspace packages missing from release manifest:" >&2
  echo "$missing" >&2
  exit 1
fi

echo "release manifest ok: $count public crates; every workspace package classified"
