---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - ram-eviction
  - nmp-core
  - pin-sets
  - view-state
supersedes: []
related_claims: []
source_lines:
  - 3378-3400
  - 3446-3482
captured_at: 2026-06-11T23:22:45Z
---

# Episode: Open-view pin sets close eviction-blank-row regression

## Prior State

PR #1096 initially pinned events on timeline IDs + event_claims keys, claiming open thread/author views were covered by event_claims — but event_claims is only populated by the separate claim_event embed mechanism, NOT by open_thread/open_author. Open thread_items() reads self.events with no store fallback; eviction would blank live thread rows.

## Trigger

Opus review (#1096) proved the pin-set gap: thread_items() scans self.events.values() with no store fallback, so evicted reply/ancestor events would vanish mid-viewing. Recovery was also broken (dedup blocks re-fetch). Same defect for non-followed author notes and author profiles.

## Decision

Added Kernel::open_view_pins() computing pins from live view state once per GC pass before any eviction: thread pins include focused id + derived root + referenced_event_ids(focused) + all four hydration bookkeeping sets (pending_ids, requested_ids, pending_reply_targets, requested_reply_targets) + every cached event matching the exact thread_items() membership predicate; author pins include every cached event by selected_author. Profile pins include selected author + all thread-participant authors.

## Consequences

- Open thread/author views survive eviction without blanking
- Requested_ids dedup hole closed — pins prevent eviction of in-flight hydration targets
- Predicate is a copy of thread_items() (not shared), creating a drift risk if thread_items membership broadens — documented in module header with #957 re-derivation obligation
- Cost: one O(events) scan per open view per 60s GC pass

## Open Tail

- #957 (retire author/thread stack) will remove the very view-state the pins read — pin derivation must be re-done when that lands

## Evidence

- transcript lines 3378-3400
- transcript lines 3446-3482

