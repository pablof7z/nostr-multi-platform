---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: root-cause
status: active
subjects:
  - gc-phase1-livelock
  - lmdb-expiration-index
  - v-118
supersedes: []
related_claims: []
source_lines:
  - 1633-1659
  - 1742-1743
captured_at: 2026-06-12T06:38:44Z
---

# Episode: Mandate real expiration index for gc livelock, not tactical hack

## Prior State

V-118 documented a gc Phase-1 livelock: `created_at` cursor with `until(T)` is inclusive, so a block of events sharing one timestamp larger than one budget pass would never be passed. The issue offered both a tactical hack and a preferred real fix.

## Trigger

User asked to dispatch documented technical debt. The assistant exercised judgment: the issue's preferred real fix (expiry_ts → event_id LMDB expiration index with backfill) makes Phase 1 O(expired) instead of O(store), which is the kind of fix the zero-debt rule exists for.

## Decision

Mandate the real fix: LMDB expiration index (`expiry_ts → event_id`) with backfill, deleting the old cursor machinery entirely. Tactical hack rejected.

## Consequences

- Phase 1 gc becomes O(expired) instead of O(store)
- Old `gc_phase1_cursor` machinery deleted entirely
- Backfill migration required for existing stores

## Open Tail

- Implementation in progress (V-118 fixer agent running)

## Evidence

- transcript lines 1633-1659
- transcript lines 1742-1743

