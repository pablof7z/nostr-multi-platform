---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: superseded
subjects:
  - nmp-store-eviction
  - nmp-core-gc
  - watermark-deletion
supersedes:
  - 2026-06-13-1-store-eviction-ceiling-re-enabled-with
related_claims: []
source_lines:
  - 8413-8453
  - 8495-8510
captured_at: 2026-06-13T20:56:22Z
---

# Episode: Store eviction: floor-coherent pins + ceiling re-enabled + watermark machinery deleted

## Prior State

Store had no eviction ceiling (HOT_EVENT_CEILING disabled at usize::MAX in GcBudget::production()). Watermark-based pinning left a middle-event hole: stored events below the floor that RAM eviction had already dropped were unprotected. Watermark machinery (read_watermark, write_watermark, coverage, WatermarkKey, WatermarkRow, SyncMethod, etc.) existed in both LMDB and mem implementations.

## Trigger

Issue #1090: unbounded store growth + middle-event hole in eviction where stored events below the shape floor could be evicted because pin derivation only considered RAM-resident events.

## Decision

Re-enable ceiling (GcBudget::production() sets max_total_events = 10,000 via HOT_EVENT_CEILING). Factor shape_floor() into a single source (ram_eviction_floor.rs). Pin stored events below the floor via pin_shape_events_below_floor() using until: Some(floor) for index-supported queries and visitor-side enforcement for Etag/Ptag. Delete all watermark machinery (trait methods, types, LMDB sub-db, mem HashMap, tests) across nmp-store, nmp-core, and nmp-testing.

## Consequences

- Store is now bounded at 10,000 events in production (tests keep usize::MAX via default())
- Floor-coherent eviction closes the middle-event hole: stored events below the floor are pinned regardless of RAM state
- All watermark tables/code removed from both LMDB and mem implementations — hard break per owner decision
- shape_floor() is a single source of truth shared between read-path and GC-path (cannot drift)
- kernel/mod.rs must stay ≤2797 LOC (floor helpers live in ram_eviction_floor.rs via #[path])

## Open Tail

*(none)*

## Evidence

- transcript lines 8413-8453
- transcript lines 8495-8510

