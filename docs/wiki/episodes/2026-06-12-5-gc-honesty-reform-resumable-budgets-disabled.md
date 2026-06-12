---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - gc
  - nmp-core
  - eviction
  - watermark
supersedes: []
related_claims: []
source_lines:
  - 3917-3924
captured_at: 2026-06-12T06:14:07Z
---

# Episode: GC honesty reform — resumable budgets, disabled LRU ceiling

## Prior State

GC was dishonest about budgets (could claim to GC without actually evicting); LRU event-count ceiling could starve new follows' backfill; snapshot perf gate had stale 250ms/150ms ceilings

## Trigger

#1085 (GC dishonesty), #1087 (watermark starving new follows), Fable review identifying both as p0

## Decision

Resumable Phase-1 cursor so GC makes real progress per tick; O(1) Phase-2 count; hourly Phase-3 tombstone gate; LRU event-count ceiling (HOT_EVENT_CEILING) explicitly disabled until store-claims are wired (#1090); perf gate tightened ~17× to 15ms/8ms.

## Consequences

- GC now genuinely evicts within its budget each tick
- New follows get backfill again (author-aware watermark rewrite in #1091)
- LRU ceiling disabled as a deliberate policy decision, tracked for re-enablement in #1090
- Cursor livelock edge case tracked in #1097

## Open Tail

- HOT_EVENT_CEILING re-enablement gated on store-claims wiring (#1090)

## Evidence

- transcript lines 3917-3924

