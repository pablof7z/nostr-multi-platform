#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

exclude_list="$(mktemp "${TMPDIR:-/tmp}/nmp-excluded-crates.XXXXXX")"
GENERATED_LOCKS=()
cleanup() {
  rm -f "$exclude_list"
  for lockfile in "${GENERATED_LOCKS[@]}"; do
    [[ -f "$lockfile" ]] && rm -f "$lockfile"
  done
}
trap cleanup EXIT

python3 - "$ROOT/Cargo.toml" > "$exclude_list" <<'PY'
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    print("python3 with tomllib is required (Python 3.11+)", file=sys.stderr)
    sys.exit(2)

manifest = Path(sys.argv[1])
data = tomllib.loads(manifest.read_text())
workspace = data.get("workspace", {})
excludes = workspace.get("exclude", [])

if not isinstance(excludes, list):
    print("[workspace].exclude must be a list", file=sys.stderr)
    sys.exit(2)

for item in excludes:
    if not isinstance(item, str):
        print("[workspace].exclude entries must be strings", file=sys.stderr)
        sys.exit(2)
    print(item)
PY

EXCLUDES=()
while IFS= read -r excluded_path; do
  EXCLUDES+=("$excluded_path")
done < "$exclude_list"

if [[ ${#EXCLUDES[@]} -eq 0 ]]; then
  echo "No [workspace].exclude entries found."
  exit 0
fi

target_root="${CARGO_TARGET_DIR:-$ROOT/target/excluded-crate-standalone}"

for excluded_path in "${EXCLUDES[@]}"; do
  manifest="$ROOT/$excluded_path/Cargo.toml"
  if [[ ! -f "$manifest" ]]; then
    echo "Missing Cargo manifest for [workspace].exclude entry: $excluded_path" >&2
    exit 1
  fi

  lockfile="$ROOT/$excluded_path/Cargo.lock"
  if [[ ! -e "$lockfile" ]]; then
    GENERATED_LOCKS+=("$lockfile")
  fi

  safe_target="${excluded_path//[^[:alnum:]._-]/_}"
  echo "Checking standalone build for excluded crate: $excluded_path"
  CARGO_TARGET_DIR="$target_root/$safe_target" cargo check --manifest-path "$manifest"
done
