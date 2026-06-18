#!/usr/bin/env bash
# run.sh — sanity-harness orchestrator (hybrid: shell drives OS metrics + relays,
# Rust bin owns the in-process counters and the absolute gates).
#
# Detects whether the CURRENT NMP architecture MISBEHAVES under real load —
# CPU pegging / busy-spin / polling, memory leaks, latency cliffs, dropped
# events, correctness breaks. Absolute thresholds, not deltas.
#
# Two relay modes:
#   (default) LOCAL: starts `nak serve --negentropy` replaying a captured corpus.
#   --live          : drives a real public relay set (still SKIPs LOUD on miss).
#
# Pipeline:
#   1. (optional) capture real events  -> artifacts/real-events.jsonl
#   2. start nak serve replaying them (local mode)
#   3. resolve a high-follow account from accounts.json (npub->hex, kind:3 count)
#   4. launch sanity-gate (signs in AS that account) in the background
#   5. align the OS sampler to each phase window; merge -> os-metrics.json
#   6. sanity-gate writes docs/perf/<run>/sanity-report.{json,md}
#
# Usage:
#   scripts/perf-sanity/run.sh [--live] [--relay URL] [--account <npub|name>] \
#       [--soak-secs 1800] [--phase all] [--run-id sanity-YYYYMMDD] [--capture]
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
cd "$REPO"

LIVE=0
RELAY=""
ACCOUNT=""
SOAK_SECS=120
PHASE="all"
RUN_ID="sanity-$(date +%Y%m%d-%H%M%S)"
DO_CAPTURE=0
LOCAL_PORT=10547

while [[ $# -gt 0 ]]; do
  case "$1" in
    --live) LIVE=1; shift;;
    --relay) RELAY="$2"; shift 2;;
    --account) ACCOUNT="$2"; shift 2;;
    --soak-secs) SOAK_SECS="$2"; shift 2;;
    --phase) PHASE="$2"; shift 2;;
    --run-id) RUN_ID="$2"; shift 2;;
    --capture) DO_CAPTURE=1; shift;;
    --port) LOCAL_PORT="$2"; shift 2;;
    -h|--help) sed -n '2,30p' "$0"; exit 0;;
    *) echo "unknown arg $1" >&2; exit 64;;
  esac
done

OUT_DIR="docs/perf/$RUN_ID"
OS_METRICS="$OUT_DIR/os-metrics.json"
mkdir -p "$OUT_DIR" artifacts
: > "$OS_METRICS"; echo '{}' > "$OS_METRICS"

# ── 1. capture (optional) ─────────────────────────────────────────────────────
if [[ "$DO_CAPTURE" == 1 ]]; then
  CAP_RELAY="${RELAY:-wss://relay.primal.net}"
  "$HERE/capture-real-events.sh" --relay "$CAP_RELAY" --limit 500 --out artifacts/real-events.jsonl
fi

# ── 2. relay setup ────────────────────────────────────────────────────────────
NAK_PID=""
cleanup() {
  [[ -n "$NAK_PID" ]] && kill "$NAK_PID" 2>/dev/null || true
  [[ -n "${SANITY_PID:-}" ]] && kill "$SANITY_PID" 2>/dev/null || true
}
trap cleanup EXIT

if [[ "$LIVE" == 1 ]]; then
  RELAY="${RELAY:-wss://relay.primal.net}"
  echo "mode: LIVE  relay=$RELAY" >&2
  LIVE_FLAG="--live"
else
  RELAY="ws://127.0.0.1:$LOCAL_PORT"
  LIVE_FLAG=""
  echo "mode: LOCAL  starting nak serve on $RELAY" >&2
  # --negentropy enables NIP-77 set-reconciliation; replay the captured corpus.
  nak serve --port "$LOCAL_PORT" --negentropy >"$OUT_DIR/nak-serve.log" 2>&1 &
  NAK_PID=$!
  sleep 2
  if [[ -s artifacts/real-events.jsonl ]]; then
    echo "seeding nak serve with $(wc -l < artifacts/real-events.jsonl) captured events" >&2
    # nak publishes each line as an EVENT to the local relay.
    while IFS= read -r line; do
      [[ "$line" == \{* ]] || continue
      echo "$line" | nak event "$RELAY" >/dev/null 2>&1 || true
    done < artifacts/real-events.jsonl
  fi
fi

# ── 3. resolve account (npub->hex, follow count) ──────────────────────────────
VIEWER_HEX=""; NSEC=""; FOLLOW_COUNT=0
if [[ -n "$ACCOUNT" && -f "$HERE/accounts.json" ]]; then
  read -r VIEWER_HEX NSEC <<EOF
$(python3 "$HERE/resolve-account.py" "$HERE/accounts.json" "$ACCOUNT")
EOF
  if [[ -n "$VIEWER_HEX" ]]; then
    echo "account: $ACCOUNT -> $VIEWER_HEX" >&2
    # Resolve the latest kind:3 p-tag count as the follow-set oracle.
    FOLLOW_COUNT=$(nak req -k 3 -a "$VIEWER_HEX" -l 1 "${RELAY/ws:/wss:}" 2>/dev/null \
      | python3 -c "import sys,json;
import functools
lines=[l for l in sys.stdin if l.strip().startswith('{')]
c=0
if lines:
    e=json.loads(lines[-1]); c=sum(1 for t in e.get('tags',[]) if t and t[0]=='p')
print(c)" 2>/dev/null || echo 0)
    echo "follow_count(kind:3 p-tags)=$FOLLOW_COUNT" >&2
  fi
fi

# ── 4. launch sanity-gate (signs in AS the account) ───────────────────────────
ARGS=(--phase "$PHASE" --relay "$RELAY" --soak-secs "$SOAK_SECS" \
      --run-id "$RUN_ID" --os-metrics "$OS_METRICS" --fail-on-gate $LIVE_FLAG)
[[ -n "$NSEC" ]] && ARGS+=(--nsec "$NSEC")
[[ -n "$VIEWER_HEX" ]] && ARGS+=(--viewer-hex "$VIEWER_HEX")
[[ "$FOLLOW_COUNT" -gt 0 ]] && ARGS+=(--follow-count "$FOLLOW_COUNT")

echo "launching: cargo run -p nmp-testing --bin sanity-gate -- ${ARGS[*]}" >&2
cargo run --release -q -p nmp-testing --bin sanity-gate -- "${ARGS[@]}" &
SANITY_PID=$!

# ── 5. align OS sampler to phase windows ──────────────────────────────────────
# Resolve the actual sanity-gate pid (cargo's child for a `cargo run`, or the
# release binary directly). Sample CONCURRENTLY with the run — the soak phases
# (idle/memory) are the spin/leak detectors that NEED these OS numbers.
sleep 3
TARGET_PID=$(pgrep -f 'sanity-gate --phase' 2>/dev/null | head -1 || true)
[[ -z "$TARGET_PID" ]] && TARGET_PID=$(pgrep -P "$SANITY_PID" sanity-gate 2>/dev/null | head -1 || true)
[[ -z "$TARGET_PID" ]] && TARGET_PID="$SANITY_PID"
echo "os-sampler target pid=$TARGET_PID" >&2

# Background a sampler per soak-class phase, each covering the whole soak window.
# `sanity-gate` reads whichever keys are present when it reaches that phase; an
# absent key → a BLOCKED row (honest), never a fake number. For per-phase
# precision against long soaks, invoke run.sh once per --phase.
SAMPLER_PIDS=()
for ph in idle_soak memory_soak firehose; do
  if [[ "$PHASE" == "all" || "$PHASE" == "$ph" ]]; then
    "$HERE/os-sampler.sh" "$TARGET_PID" "$ph" "$SOAK_SECS" 1 "$OS_METRICS" &
    SAMPLER_PIDS+=("$!")
  fi
done

wait "$SANITY_PID"; RC=$?
for sp in "${SAMPLER_PIDS[@]:-}"; do [[ -n "$sp" ]] && wait "$sp" 2>/dev/null || true; done
echo "sanity-gate exited rc=$RC; report: $OUT_DIR/sanity-report.md" >&2
exit "$RC"
