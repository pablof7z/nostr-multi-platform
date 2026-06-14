---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: root-cause
status: superseded
subjects:
  - feed-idle-validation
  - root-ingestion
  - false-resend-probe
  - ffi-stress-capstone
supersedes: []
related_claims: []
source_lines:
  - 10560-10770
captured_at: 2026-06-14T16:20:53Z
---

# Episode: Feed idle omission validated at 97.6%; OP-centric root ingestion not follow-gated

## Prior State

R3-S5 capstone could not measure the real whole-product idle win because the feed projection (nmp.feed.home, the dominant ~41KB/tick payload) was never wired in the harness. The false-resend probe was assumed to test the over-invalidation risk. It was believed that stranger pubkeys would be filtered by the follow predicate before reaching the engine.

## Trigger

R6-S4 capstone registered op_feed in ffi-stress and measured the real idle win. Opus review identified that the false-resend probe tested only the trivial case (stranger pubkey rejected before reaching the engine). Investigation then revealed that the OP-centric engine's ingest_root is NOT follow-gated — it surfaces all roots regardless of author — and RootFeedSnapshot carries total_blocks, so any new root legitimately changes bytes and should re-emit.

## Decision

Redesigned the false-resend probe to test a followed-author reply to a root the engine never holds (passes predicate, mutates internal pending_attributions, but leaves total_blocks unchanged → byte-identical → must omit). Retained the stranger probe as an informational secondary. Added nightly CI gate (ffi-stress feed-idle --fail-on-gate). Narrowed MiniProjectionCache doc to 'steady-state subset' (does not model session/epoch rebaseline, which is proven by S1's FrameIdentity tests).

## Consequences

- 97.6% idle total-frame-byte reduction empirically validated: 45,440 B → 1,104 B (44,336 B saved per idle tick)
- The over-invalidation proof is genuine: 0/1 followed out-of-window events caused false re-emission
- Discovery that ingest_root is not follow-gated changes understanding of the engine's content pipeline — stranger roots do enter the feed and legitimately change total_blocks
- Nightly CI regression gate will catch future breakage of feed omission

## Open Tail

- R6-S5 release/device jank measurement still pending — validates whether felt jank is actually fixed
- The feed still re-sends the whole payload on a mutating in-window event (Option B row-deltas deferred)

## Evidence

- transcript lines 10560-10770

