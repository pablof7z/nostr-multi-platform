---
type: episode-card
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
salience: architecture
status: active
subjects:
  - nmp-core-gc
  - nmp-store-eviction
  - watermark-machinery
supersedes:
  - 2026-06-13-2-store-eviction-floor-coherent-pins-ceiling
related_claims: []
source_lines:
  - 9142-9143
captured_at: 2026-06-13T21:35:37Z
---

# Episode: Floor-coherent eviction replaces dead watermark machinery

## Prior State

Persisted watermark (WatermarkKey/WatermarkRow/SyncMethod/Coverage) had zero production writers; HOT_EVENT_CEILING was usize::MAX, meaning eviction was effectively disabled and storage unbounded

## Trigger

Architecture audit found watermark machinery was dead code with no writers and GC was non-functional; owner decision #1090 to replace with content-derived eviction

## Decision

Replace watermark-based eviction with floor-coherent eviction: new ram_eviction_floor.rs module with shape_floor() + pin_shape_events_below_floor(); delete all dead watermark machinery (WatermarkKey/WatermarkRow/SyncMethod/Coverage, read_watermark/write_watermark/coverage trait methods); set HOT_EVENT_CEILING to 10000

## Consequences

- GC now bounds storage at 10K events (HOT_EVENT_CEILING)
- Eviction is content-derived (floor) not persisted (watermark)
- Deleted ~5 watermark-related types and trait methods
- Shipped via #1327 merged to master

## Open Tail

*(none)*

## Evidence

- transcript lines 9142-9143

