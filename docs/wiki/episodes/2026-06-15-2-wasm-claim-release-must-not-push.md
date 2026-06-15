---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - wasm-dispatch
  - claim-release-snapshot
  - solidjs-remount-loop
supersedes:
  - 2026-06-15-2-wasm-dispatch-must-not-push-snapshots
related_claims: []
source_lines:
  - 2388-2418
captured_at: 2026-06-15T04:23:17Z
---

# Episode: Wasm claim/release must not push snapshots — SolidJS remount loop root cause

## Prior State

The wasm dispatch arm pushed a fresh snapshot frame on every claim_profile/release_profile call. This was not an issue in the native actor (which handles claim/release as refcount bookkeeping only).

## Trigger

Web feed CI regression after the profile-claim registry migration: SolidJS <For> rebuilds its rows on each snapshot → remounts NostrAvatar/NostrProfileName → their onMount/onCleanup re-dispatch claim/release → another snapshot → infinite loop. Worker instrumentation proved 170k+ snapshot frames and 16k+ alternating claim/release calls, OOM-crashing the renderer or starving it so names/avatars never resolved.

## Decision

Claim/release dispatch now ACK with ActionAccepted only and pushes no snapshot. The resolved kind:0 arrives via the relay-pool ingest sink, which pushes its own snapshot. This mirrors the native actor pattern. Established as a general architectural rule for future wasm dispatch arms.

## Consequences

- Feed.spec.ts 10/10, Playwright 3/3, vitest 42/42, cargo nmp-core/nmp-wasm green
- Native regression guard added (claim_no_snapshot_tests) asserting claim/release emit ActionAccepted with no UpdateBytes
- General wasm-dispatch rule: claim/release are refcount bookkeeping carrying no new user-visible data — never push snapshots for them

## Open Tail

*(none)*

## Evidence

- transcript lines 2388-2418
