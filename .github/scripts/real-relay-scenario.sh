#!/usr/bin/env bash
set -uo pipefail

: "${SCENARIO:?SCENARIO is required}"
: "${CAPABILITY:?CAPABILITY is required}"
: "${RELAYS:?RELAYS is required}"
: "${COMMAND:?COMMAND is required}"

OUTPUT_DIR="${OUTPUT_DIR:-real-relay-output}"
mkdir -p "$OUTPUT_DIR"

SAFE_SCENARIO="$(printf '%s' "$SCENARIO" | tr -c 'A-Za-z0-9._-' '-')"
OUTPUT_FILE="$OUTPUT_DIR/${SAFE_SCENARIO}.txt"
REACHABILITY_FILE="$OUTPUT_DIR/${SAFE_SCENARIO}-reachability.md"

append_summary() {
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      echo "$1"
      echo
    } >> "$GITHUB_STEP_SUMMARY"
  fi
}

probe_relay() {
  local relay="$1"
  local probe_url="$relay"
  local code
  probe_url="${probe_url/#wss:\/\//https://}"
  probe_url="${probe_url/#ws:\/\//http://}"
  code="$(curl -sS -o /dev/null -w "%{http_code}" \
    --max-time 10 \
    -H "Upgrade: websocket" \
    -H "Connection: Upgrade" \
    -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
    -H "Sec-WebSocket-Version: 13" \
    "$probe_url" 2>/dev/null || true)"
  printf '%s' "${code:-000}"
}

{
  echo "### Relay Reachability"
  echo
  echo "| Relay | HTTP probe | Status |"
  echo "|---|---:|---|"
} > "$REACHABILITY_FILE"

reachable=0
for relay in $RELAYS; do
  code="$(probe_relay "$relay")"
  if [ -n "$code" ] && [ "$code" != "000" ]; then
    reachable=$((reachable + 1))
    status="reachable"
  else
    status="unreachable"
  fi
  echo "| \`$relay\` | \`$code\` | $status |" >> "$REACHABILITY_FILE"
done

cat "$REACHABILITY_FILE"

if [ "$reachable" -eq 0 ]; then
  {
    echo "SKIP: $SCENARIO has no reachable relay candidates."
    echo
    cat "$REACHABILITY_FILE"
  } | tee "$OUTPUT_FILE"
  append_summary "## $SCENARIO - SKIP

Capability: \`$CAPABILITY\`

No relay candidate was reachable, so the public-relay scenario was skipped before running Cargo.

Command not run:

\`\`\`bash
$COMMAND
\`\`\`"
  echo "scenario_status=SKIP" >> "${GITHUB_OUTPUT:-/dev/null}"
  exit 0
fi

append_summary "## $SCENARIO - running

Capability: \`$CAPABILITY\`

Reachable relay candidates: \`$reachable\`

\`\`\`bash
$COMMAND
\`\`\`"

echo "COMMAND: $COMMAND" | tee "$OUTPUT_FILE"
set +e
bash -lc "$COMMAND" 2>&1 | tee -a "$OUTPUT_FILE"
status="${PIPESTATUS[0]}"
set -e

if [ "$status" -ne 0 ]; then
  verdict="FAIL"
elif grep -Eq '(^|[[:space:]])SKIP[: ]' "$OUTPUT_FILE"; then
  verdict="SKIP"
elif grep -Eq '^NIP-77 error:' "$OUTPUT_FILE"; then
  verdict="FAIL"
else
  verdict="PASS"
fi

append_summary "## $SCENARIO - $verdict

Capability: \`$CAPABILITY\`

Relays: \`$RELAYS\`

Exit status: \`$status\`

Output: \`$OUTPUT_FILE\`"

echo "scenario_status=$verdict" >> "${GITHUB_OUTPUT:-/dev/null}"

if [ "$verdict" = "FAIL" ]; then
  exit 1
fi
