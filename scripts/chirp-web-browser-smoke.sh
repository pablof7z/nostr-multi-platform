#!/usr/bin/env bash
set -euo pipefail

URL="${1:-http://127.0.0.1:4173/}"
SESSION="chirp-web-smoke-$$"

cleanup() {
  agent-browser --session "$SESSION" close >/dev/null 2>&1 || true
}
trap cleanup EXIT

agent-browser --session "$SESSION" open "$URL" >/dev/null
agent-browser --session "$SESSION" wait --text "Home timeline" >/dev/null
agent-browser --session "$SESSION" wait --text "Start worker" >/dev/null
agent-browser --session "$SESSION" find role button click --name "Start worker" >/dev/null
agent-browser --session "$SESSION" wait --text "browser bridge unavailable" >/dev/null
agent-browser --session "$SESSION" find placeholder "What is happening on Nostr?" fill "browser smoke" >/dev/null
agent-browser --session "$SESSION" find role button click --name "Publish" >/dev/null
agent-browser --session "$SESSION" wait --text "capability_failure" >/dev/null

echo "Chirp web browser smoke passed at $URL"
