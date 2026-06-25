#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

exclude_list="$(mktemp "${TMPDIR:-/tmp}/nmp-excluded-crates.XXXXXX")"
GENERATED_LOCKS=()
cleanup() {
  rm -f "$exclude_list"
  if ((${#GENERATED_LOCKS[@]})); then
    for lockfile in "${GENERATED_LOCKS[@]}"; do
      [[ -f "$lockfile" ]] && rm -f "$lockfile"
    done
  fi
}
trap cleanup EXIT

sed -n '/^exclude = \[/,/^\]/p' "$ROOT/Cargo.toml" \
  | sed -n 's/^[[:space:]]*"\([^"]*\)".*/\1/p' \
  > "$exclude_list"

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
