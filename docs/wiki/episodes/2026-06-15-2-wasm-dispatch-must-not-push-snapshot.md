---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-wasm
  - wasm-dispatch
  - solidjs-web-feed
supersedes:
  - 2026-06-15-2-wasm-claim-release-must-not-push
related_claims: []
source_lines:
  - 2389-2418
captured_at: 2026-06-15T04:33:40Z
---

# Episode: Wasm dispatch must not push snapshot on claim/release — infinite SolidJS remount loop

## Prior State

The wasm dispatch arm pushed a fresh snapshot frame on every action including claim_profile/release_profile. Claim/release were treated like any other action that should trigger a UI snapshot update.

## Trigger

Profile-claim registry migration exposed an unbounded UI loop on the web: wasm dispatch pushed snapshot on claim → SolidJS <For> rebuilds rows → remounts NostrAvatar/NostrProfileName → onMount/onCleanup re-dispatch claim/release → another snapshot → infinite loop (170k+ snapshot frames, 16k+ alternating claim/release calls, OOM/starvation causing profile resolution to never complete). Reproduced consistently: PR branch 0/8 passed, master 2/5 passed (only tail flake).

## Decision

Claim/release now ACK with ActionAccepted and push no snapshot. The resolved kind:0 arrives via the relay-pool ingest sink, which pushes its own snapshot. This mirrors the native actor behavior. Added native regression guard (claim_no_snapshot_tests) asserting claim/release emit ActionAccepted with no UpdateBytes.

## Consequences

- Established as a general rule for future wasm dispatch arms: claim/release are refcount bookkeeping carrying no new user-visible data, so they must not push snapshots
- Web feed tests went from 0/8 to 10/10 pass
- Required file-size split of runtime.rs (582→518 LOC, dispatch.rs extracted) to stay under baseline hard-cap

## Open Tail

*(none)*

## Evidence

- transcript lines 2389-2418
