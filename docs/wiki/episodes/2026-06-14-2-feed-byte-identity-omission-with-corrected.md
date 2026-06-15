---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: active
subjects:
  - adr-0055-feed-gating
  - incremental-emission
  - byte-identity-gate
supersedes: []
related_claims: []
source_lines:
  - 10560-10755
captured_at: 2026-06-14T20:54:15Z
---

# Episode: Feed byte-identity omission with corrected over-invalidation proof

## Prior State

Home feed emitted ~41KB every 4Hz tick regardless of change; prior false-resend gate tested only stranger-pubkey events (trivially rejected by follow predicate before reaching the feed engine)

## Trigger

Opus review identified that the false-resend gate tested the wrong thing — stranger events never reach the engine; investigation then revealed that the OP-centric engine's ingest_root is NOT follow-gated (only replies are) and RootFeedSnapshot carries total_blocks, so any new root legitimately changes bytes and should re-emit

## Decision

Feed projection omitted on exact-byte-identity keyed by (session_id, snapshot_epoch); false-resend probe corrected to test a followed-author reply-to-unknown-root (the genuine over-invalidation case — passes predicate, mutates internal state, but snapshot byte-identical); nightly CI gate wired for regression

## Consequences

- 97.6% idle frame-byte reduction (45,440B → 1,104B; feed payload 41,112B → 0 omitted)
- Corrected understanding: roots are NOT follow-gated and total_blocks forces legitimate re-emit for any new root — stranger probe retained as informational predicate-sanity check only
- MiniProjectionCache oracle documented as steady-state subset only (not session/epoch rebaseline)
- Standing nightly CI gate now catches regressions in feed omission

## Open Tail

- Out-of-window followed-root re-emit is correct behavior (total_blocks changes), not a bug — any future optimization would need row-deltas (Option B, currently deferred)

## Evidence

- transcript lines 10560-10755
