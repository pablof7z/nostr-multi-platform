#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${1:-$ROOT/release/nmp-release.toml}"

if [[ ! -f "$MANIFEST" ]]; then
  echo "missing release manifest: $MANIFEST" >&2
  exit 1
fi

manifest_string() {
  local key="$1"
  awk -F '"' -v key="$key" '$0 ~ "^" key " = " { print $2; exit }' "$MANIFEST"
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

version="${NMP_RELEASE_VERSION:-$(workspace_version)}"
tag_pattern="$(manifest_string tag_pattern)"
note_pattern="$(manifest_string migration_note_pattern)"

if [[ -z "$version" ]]; then
  echo "could not read release version from Cargo.toml" >&2
  exit 1
fi

if [[ -z "$tag_pattern" ]]; then
  echo "release manifest must declare tag_pattern" >&2
  exit 1
fi

if [[ -z "$note_pattern" ]]; then
  echo "release manifest must declare migration_note_pattern" >&2
  exit 1
fi

tag="${tag_pattern//\{version\}/$version}"
note_rel="${note_pattern//\{version\}/$version}"
note_rel="${note_rel//\{tag\}/$tag}"

if [[ "$note_rel" = /* || "$note_rel" == *".."* ]]; then
  echo "migration_note_pattern must resolve to a repository-relative path: $note_rel" >&2
  exit 1
fi

note="$ROOT/$note_rel"
if [[ ! -f "$note" ]]; then
  echo "missing migration note for $tag: $note_rel" >&2
  exit 1
fi

if ! grep -q "$tag" "$note"; then
  echo "migration note $note_rel must name release tag $tag" >&2
  exit 1
fi

required_sections=(
  "## Deleted Or Renamed Crates And APIs"
  "## Projection Keys And Schema IDs"
  "## Dispatch Envelopes And Actions"
  "## UniFFI And Binding Changes"
  "## Consumer Checklist"
)

for section in "${required_sections[@]}"; do
  if ! grep -Fxq "$section" "$note"; then
    echo "migration note $note_rel missing required section: $section" >&2
    exit 1
  fi
done

echo "release migration note ok: $note_rel"
