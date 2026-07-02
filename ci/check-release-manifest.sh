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

if ! grep -q '^migration_note_pattern = ' "$MANIFEST"; then
  echo "release manifest must declare migration_note_pattern" >&2
  exit 1
fi

if ! grep -q '  "bash ci/check-release-migration-note.sh",' "$MANIFEST"; then
  echo "release manifest required_gates must include bash ci/check-release-migration-note.sh" >&2
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

extract_public_npm_packages() {
  awk -F '"' '
    /^\[\[public_npm_packages\]\]/ { in_public = 1; name = ""; path = ""; next }
    /^\[\[/ && $0 !~ /^\[\[public_npm_packages\]\]/ { in_public = 0 }
    in_public && /^name = / { name = $2 }
    in_public && /^path = / { path = $2 }
    in_public && name != "" && path != "" {
      print name "|" path
      in_public = 0
    }
  ' "$MANIFEST"
}

workspace_version() {
  awk '
    /^\[workspace.package\]/ { in_workspace_package = 1; next }
    /^\[/ && in_workspace_package { in_workspace_package = 0 }
    in_workspace_package && /^version = / {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "$ROOT/Cargo.toml"
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
npm_count=0
classified="$(mktemp)"
npm_classified="$(mktemp)"
workspace="$(mktemp)"
trap 'rm -f "$classified" "$npm_classified" "$workspace"' EXIT
release_version="$(workspace_version)"

if [[ -z "$release_version" ]]; then
  echo "could not read workspace package version from Cargo.toml" >&2
  exit 1
fi

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

while IFS='|' read -r name relpath; do
  [[ -n "$name" ]] || continue
  npm_count=$((npm_count + 1))
  printf '%s\t%s\n' "$name" "$relpath" >> "$npm_classified"
  package_json="$ROOT/$relpath/package.json"
  if [[ ! -f "$package_json" ]]; then
    echo "public npm package $name points at missing manifest: $relpath/package.json" >&2
    exit 1
  fi
  actual_name="$(jq -r '.name // ""' "$package_json")"
  if [[ "$actual_name" != "$name" ]]; then
    echo "public npm package $name path $relpath has a different package name: $actual_name" >&2
    exit 1
  fi
  actual_version="$(jq -r '.version // ""' "$package_json")"
  if [[ "$actual_version" != "$release_version" ]]; then
    echo "public npm package $name must use workspace release version $release_version (found $actual_version)" >&2
    exit 1
  fi
  if [[ "$(jq -r '.private // false' "$package_json")" == "true" ]]; then
    echo "public npm package $name is marked private" >&2
    exit 1
  fi
  if ! jq -e '.publishConfig.access == "public"' "$package_json" >/dev/null; then
    echo "public npm package $name must declare publishConfig.access = public" >&2
    exit 1
  fi
  if ! jq -e '.files | index("dist")' "$package_json" >/dev/null; then
    echo "public npm package $name must publish dist/" >&2
    exit 1
  fi
  if ! jq -e '.main and .types and .exports["."] and .scripts.build and .scripts.prepack' "$package_json" >/dev/null; then
    echo "public npm package $name must declare main/types/root export/build/prepack" >&2
    exit 1
  fi
  if [[ "$name" == "@nmpis/runtime-web" ]] && ! jq -e '.exports["./worker"]' "$package_json" >/dev/null; then
    echo "public npm package $name must export ./worker" >&2
    exit 1
  fi
done < <(extract_public_npm_packages)

if [[ "$count" -eq 0 ]]; then
  echo "release manifest declares no public crates" >&2
  exit 1
fi

if [[ "$npm_count" -eq 0 ]]; then
  echo "release manifest declares no public npm packages" >&2
  exit 1
fi

duplicates="$(sort "$classified" | uniq -d)"
if [[ -n "$duplicates" ]]; then
  echo "packages classified more than once:" >&2
  echo "$duplicates" >&2
  exit 1
fi

npm_duplicates="$(sort "$npm_classified" | uniq -d)"
if [[ -n "$npm_duplicates" ]]; then
  echo "npm packages classified more than once:" >&2
  echo "$npm_duplicates" >&2
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

echo "release manifest ok: $count public crates; $npm_count public npm packages; every workspace package classified"
