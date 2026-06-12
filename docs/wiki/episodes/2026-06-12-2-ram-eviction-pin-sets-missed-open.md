---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - ram-eviction
  - pin-sets
  - open-view-safety
supersedes: []
related_claims: []
source_lines:
  - 3378-3410
captured_at: 2026-06-12T00:32:21Z
---

# Episode: RAM Eviction Pin Sets Missed Open View Working Sets

## Prior State

Initial #1096 implementation pinned events only by timeline membership and event_claims keys, claiming that open thread/author views were covered by event_claims.

## Trigger

Opus review of #1096 proved that event_claims is populated only by the separate claim_event embed mechanism — open_thread/open_author paths set thread_view.selected_thread / author_view.selected_author (ViewInterest refcounts) and write nothing to event_claims. Eviction would blank live thread rows silently, and recovery was also broken (evicted IDs stayed in requested_ids, dedup blocked re-fetch, and read paths never fell back to LMDB).

## Decision

Pin sets re-derived from live view state: open thread pins focused+root+referenced_event_ids+all four hydration bookkeeping sets+every cached event matching thread_items() predicate; open author pins every cached event by selected author; profiles pin selected author + thread participant authors. Computed once per GC pass before any eviction via Kernel::open_view_pins().

## Consequences

- Pin derivation is a copy of the thread_items() predicate (drift risk — future broadening won't auto-propagate)
- #957 legacy-stack deletion will require re-derivation from interest registry
- Re-derivation obligation documented in ram_eviction.rs module header

## Open Tail

- Extract shared membership predicate to eliminate drift risk
- Re-derive pins from lifecycle.registry().iter_active() when #957 lands

## Evidence

- transcript lines 3378-3410

