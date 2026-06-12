---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - gc-perf-gate
  - snapshot-budgets
supersedes: []
related_claims: []
source_lines:
  - 3411-3430
captured_at: 2026-06-12T00:32:21Z
---

# Episode: GC Perf Gate Thresholds Were ~17× Too Loose

## Prior State

GC perf gate thresholds were 250ms/150ms (make_update/serialize), presumed adequate with wide margin.

## Trigger

#1085 fix agent's own re-run produced make_update_us=1618µs under contention, leaving only 3.7× margin under the old 6000µs ceiling — confirming the reviewer's prediction that shared-runner variance would cause flaky failures.

## Decision

Thresholds tightened to 15ms/8ms (MAX_MAKE_UPDATE_US / MAX_SERIALIZE_US) — approximately 17× tighter. Rationale comments cite measured under-contention values (1330–1342µs / ~630µs) and the ~10× margin logic. Same-created_at cursor livelock filed as #1097 (V-118).

## Consequences

- Perf gate is no longer flaky on shared runners
- LRU event-count ceiling (HOT_EVENT_CEILING) disabled until store-claims are wired (#1090)
- Cursor livelock edge case tracked as known limitation V-118

## Open Tail

- #1097: durable fix for same-created_at cursor livelock (exclusive bound + no-progress detector or expiration index)

## Evidence

- transcript lines 3411-3430

