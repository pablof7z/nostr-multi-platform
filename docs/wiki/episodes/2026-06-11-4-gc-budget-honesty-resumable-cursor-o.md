---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - gc-step
  - nmp-store
  - lru-ceiling
  - eviction-budget
supersedes: []
related_claims: []
source_lines:
  - 3203-3244
  - 3411-3428
captured_at: 2026-06-11T23:22:45Z
---

# Episode: GC budget honesty: resumable cursor, O(1) count, LRU ceiling disabled

## Prior State

GC phase-1 scanned all events from the top every tick (no resume cursor), phase-2 counted events via O(N) full scan, phase-3 ran tombstone purge every tick, and the LRU ceiling was in the production budget with no production claim/release callers.

## Trigger

Finding (#1085) that GC was not honestly budgeted — scans were unbudgeted, counts were O(N), and the LRU ceiling was enforced but never actually protected live data (no claim mechanism existed).

## Decision

Phase-1: resumable cursor (gc_phase1_cursor storing last-scanned created_at, resumes with Filter::until). Phase-2: O(1) event count via ci_index.len. Phase-3: hourly tombstone purge gate. LRU ceiling disabled entirely (GcBudget::production() returns max_total_events = usize::MAX) until EventStore::claim/release have production callers (#1090). Perf gates tightened 40-50× (250ms→15ms/6ms→8ms after reviewer flake-risk pushback).

## Consequences

- GC now completes in ≤4× budget per tick
- Store grows unbounded until #1090 wires claims
- Same-created_at cursor livelock identified and filed as V-118 (#1097)
- Perf gate survived 1618µs under contention (9.3× margin at new threshold)

## Open Tail

- #1090 must wire claim/release before LRU ceiling can be re-enabled
- #1097 same-created_at cursor livelock needs durable fix

## Evidence

- transcript lines 3203-3244
- transcript lines 3411-3428

