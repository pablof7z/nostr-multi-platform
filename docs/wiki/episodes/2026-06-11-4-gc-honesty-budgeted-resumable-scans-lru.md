---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - gc-budget
  - nmp-store
  - lmdb
  - lru-ceiling
supersedes: []
related_claims: []
source_lines:
  - 3202-3247
captured_at: 2026-06-11T23:31:21Z
---

# Episode: GC honesty — budgeted resumable scans, LRU ceiling disabled

## Prior State

GC step was unbudgeted: O(N) full scans with no duration gate, O(N) event count, tombstone purge every cycle, LRU ceiling was active in production but had no claim/release callers.

## Trigger

Bug #1085 — GC was dishonest about budgets, could block the main thread with full scans.

## Decision

Phase-1 reaper now budgeted with resumable cursor (`Filter::until(cursor)`, `gc_phase1_cursor` on `Inner`). Phase-2 count is O(1) via `ci_index.len()`. Phase-3 tombstone purge gated hourly. LRU ceiling disabled (`max_total_events = usize::MAX`) until `claim/release` have production callers (#1090). Perf gates tightened 40–50× (250ms→6ms ceiling, then 15ms/8ms after review).

## Consequences

- GC now budgeted per-tick, resumable across cycles
- Same-created_at cursor livelock identified as #1097 (V-118)
- #1090 tracks LRU re-enable requirement with live-reference reasoning obligation

## Open Tail

- #1090 — wire store-claims from live projections, then re-enable HOT_EVENT_CEILING
- #1097 — same-created_at cursor livelock in Phase-1 reaper

## Evidence

- transcript lines 3202-3247

