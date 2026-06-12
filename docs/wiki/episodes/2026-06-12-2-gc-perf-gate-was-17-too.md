---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - gc-budget
  - perf-gate
  - cursor-livelock
supersedes: []
related_claims: []
source_lines:
  - 3411-3430
captured_at: 2026-06-12T00:59:06Z
---

# Episode: GC perf gate was 17× too loose; cursor livelock discovered

## Prior State

GC perf thresholds were 250ms/150ms (stale ceilings from early development); `gc_step` had no resumable cursor — it ran to completion or not at all; same-`created_at` entries could cause a cursor livelock

## Trigger

Reviewer required thresholds of 15ms/8ms based on measured under-contention values (1330–1342µs / ~630µs with ~10× margin). The agent's own re-run confirmed: 1618µs under contention left only 3.7× margin under the old 6000µs ceiling, proving the flake prediction correct.

## Decision

Tightened perf gate ~17× (15ms/8ms from 250ms/150ms). GC rewritten with resumable Phase-1 cursor, O(1) Phase-2 count, hourly Phase-3 tombstone gate. LRU event-count ceiling (`HOT_EVENT_CEILING`) disabled until store-claims are wired (#1090). Same-`created_at` cursor livelock tracked as #1097 (V-118, priority p3).

## Consequences

- GC runs with honest, measured budgets — no more stale ceilings that could mask regressions
- Resumable cursor means GC can make progress within budget rather than all-or-nothing
- LRU ceiling disabled means no count-based eviction until store-claims provide correct pin semantics
- Cursor livelock on same-`created_at` entries is a known edge case (exclusive bound + no-progress detector as tactical fix, expiration index as durable fix)

## Open Tail

- #1090 for re-enabling LRU ceiling with store-claims
- #1097 for the cursor livelock fix

## Evidence

- transcript lines 3411-3430

