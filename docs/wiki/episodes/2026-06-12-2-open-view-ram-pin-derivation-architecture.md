---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - ram-eviction
  - gc
  - open-view-pins
  - nmp-core
supersedes: []
related_claims: []
source_lines:
  - 3449-3478
  - 3652-3667
captured_at: 2026-06-12T06:14:07Z
---

# Episode: Open-view RAM pin derivation — architecture doctrine for eviction safety

## Prior State

No RAM eviction bounds existed; open views (threads, author feeds) had no protection against their data being evicted during GC, risking silent data loss in active UI

## Trigger

#1088 kernel memory bounds issue; reviewer demanded that pin derivation cover open thread/author views, including pending/requested hydration sets that close a broken-recovery dedup hole

## Decision

Pin sets derived from `open_view_pins()` computing from live view state before each GC pass. Events pinned when id is in: timeline (≤500), event_claims, open thread view (focused + derived root + referenced_event_ids + all four hydration bookkeeping sets + every cached event matching thread_items() membership predicate), open author view (every cached event whose author matches). Profiles pinned by: timeline_authors ∪ profile_claims ∪ active_account ∪ open-view authors. Later re-derived via `lifecycle.registry().iter_active()` + `shape.matches_event_with_id()` when the original view-state references were deleted in #1100.

## Consequences

- Open-view data survives eviction; 4 new sharp tests verified (fail with pins disabled)
- Pin derivation was later re-derived when #1100 deleted the view-state structs it read from — requiring migration to the interest-registry model
- Sub-millisecond cost at 1000-entry HWM (one O(events) scan per open view per 60s GC pass, zero when no view open)
- Reviewers flagged predicate-copy drift risk (thread_items membership predicate duplicated, not shared) — recorded as re-derivation obligation for #957's retirement

## Open Tail

- Store LRU ceiling (HOT_EVENT_CEILING) still disabled, tracked in #1090
- Cursor livelock edge case tracked in #1097

## Evidence

- transcript lines 3449-3478
- transcript lines 3652-3667

