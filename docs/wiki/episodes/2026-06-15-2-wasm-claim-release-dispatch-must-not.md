---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-wasm
  - claim-release-snapshot
  - web-feed-loop
supersedes: []
related_claims: []
source_lines:
  - 2388-2412
  - 2473-2489
captured_at: 2026-06-15T02:57:43Z
---

# Episode: Wasm claim/release dispatch must not push snapshots (infinite loop doctrine)

## Prior State

The wasm dispatch arm pushed a fresh UpdateBytes snapshot frame on every claim_profile / release_profile call. This was acceptable before the migration because claims were infrequent; the registry migration made claims frequent and coordinated (UI components mount → claim, unmount → release).

## Trigger

PR #1436 caused the web Playwright feed test to fail consistently (3/3 on CI, 0/8 locally on the branch vs 8/8 green on master). Investigation of the browser logs revealed 170k+ snapshot frames and 16k+ alternating claim/release calls — an infinite render loop: claim → snapshot → SolidJS <For> rebuilds rows → remount NostrAvatar/NostrProfileName → onMount re-dispatches claim → snapshot → loop → OOM/starvation, preventing names/avatars from ever rendering.

## Decision

claim/release now ACK with ActionAccepted only and push NO snapshot. Claim/release are refcount bookkeeping carrying no new user-visible data; the resolved kind:0 arrives via the relay-pool ingest sink, which pushes its own snapshot. This mirrors the native actor pattern. Added claim_no_snapshot_tests as a native regression guard asserting no UpdateBytes on claim/release.

## Consequences

- Web feed renders correctly (feed.spec.ts 10/10, full Playwright 3/3, vitest 42/42)
- Architectural invariant established: wasm claim/release is refcount bookkeeping only — never a snapshot trigger
- File-size gate required extracting dispatch.rs (the action-namespace routing arm) from runtime.rs into a sibling file (582 → 518 LOC)
- The earlier reconnect-gating fix (clear_probed_mailboxes only on genuine reconnects) is complementary but was not alone sufficient — the snapshot loop was the decisive breakage

## Open Tail

*(none)*

## Evidence

- transcript lines 2388-2412
- transcript lines 2473-2489
