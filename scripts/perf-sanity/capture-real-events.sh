#!/usr/bin/env bash
# capture-real-events.sh — dump real Nostr events to artifacts/real-events.jsonl
# via `nak req`, for the controlled local firehose (replayed by `nak serve`).
#
# Captures the kinds the sanity firehose + correctness oracles care about:
#   1 (note) 6 (repost) 7 (reaction) 0 (profile) 3 (contacts)
#   10002 (relay list) 1059 (gift-wrap) 30023 (long-form) 9735 (zap receipt)
#
# Usage:
#   capture-real-events.sh [--relay wss://relay.primal.net] [--limit 500] \
#                          [--authors <hex,hex,...>] [--out artifacts/real-events.jsonl]
#
# With --authors (e.g. a 2k-follow account's follow set) the capture is the real
# follow-feed corpus; without it, a broad public sample.
set -euo pipefail

RELAY="wss://relay.primal.net"
LIMIT=500
AUTHORS=""
OUT="artifacts/real-events.jsonl"
KINDS=(1 6 7 0 3 10002 1059 30023 9735)

while [[ $# -gt 0 ]]; do
  case "$1" in
    --relay) RELAY="$2"; shift 2;;
    --limit) LIMIT="$2"; shift 2;;
    --authors) AUTHORS="$2"; shift 2;;
    --out) OUT="$2"; shift 2;;
    *) echo "unknown arg $1" >&2; exit 64;;
  esac
done

command -v nak >/dev/null || { echo "nak not on PATH (https://github.com/fiatjaf/nak)" >&2; exit 1; }
mkdir -p "$(dirname "$OUT")"
: > "$OUT"

author_args=()
if [[ -n "$AUTHORS" ]]; then
  IFS=',' read -ra A <<< "$AUTHORS"
  for a in "${A[@]}"; do author_args+=(-a "$a"); done
fi

for k in "${KINDS[@]}"; do
  echo "capture: kind:$k from $RELAY (limit $LIMIT)" >&2
  # `nak req` prints one event JSON per line — exactly the jsonl shape the
  # firehose phase reads. Failures on a single kind must not abort the capture.
  nak req -k "$k" -l "$LIMIT" "${author_args[@]}" "$RELAY" 2>/dev/null >> "$OUT" || \
    echo "  (kind:$k returned nothing or relay refused)" >&2
done

count=$(wc -l < "$OUT" | tr -d ' ')
echo "capture: wrote $count events to $OUT" >&2
