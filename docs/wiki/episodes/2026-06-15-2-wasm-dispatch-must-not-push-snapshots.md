---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - wasm-dispatch
  - solidjs
  - claim-release-snapshot
  - web-feed-regression
supersedes:
  - 2026-06-15-2-wasm-claim-release-dispatch-snapshot-per
related_claims: []
source_lines:
  - 2398-2412
captured_at: 2026-06-15T03:56:11Z
---

# Episode: Wasm dispatch must not push snapshots on claim/release — SolidJS remount loop

## Prior State

The wasm dispatch arm pushed a fresh snapshot frame on every claim_profile and release_profile call. This was benign when claims were infrequent (only a few surfaces claimed).

## Trigger

After the profile-claim migration made every UI surface claim profiles (avatars, mentions, attributions), the snapshot-per-claim caused an infinite loop in the web (single-threaded wasm) build: SolidJS <For> rebuilds rows on each snapshot → remounts NostrAvatar/NostrProfileName → onMount/onCleanup re-dispatches claim/release → another snapshot → loop (170k+ frames, 16k+ alternating claim/release calls, OOM/starvation). Feed test rendered 0 posts.

## Decision

claim/release dispatch now ACKs with ActionAccepted and pushes no snapshot. The resolved kind:0 data arrives via the relay-pool ingest sink which pushes its own snapshot independently. This mirrors the native actor behavior. Native regression guard (claim_no_snapshot_tests) added asserting no UpdateBytes on claim/release.

## Consequences

- Web feed renders correctly; the loop is broken at its engine (no snapshot = no remount trigger)
- Established as a general architectural rule for future wasm dispatch arms: refcount bookkeeping must not push data snapshots
- The earlier reconnect-churn fix (gate clear_probed_mailboxes to genuine reconnects) was complementary but not sufficient alone — the snapshot loop was the decisive regression

## Open Tail

*(none)*

## Evidence

- transcript lines 2398-2412
