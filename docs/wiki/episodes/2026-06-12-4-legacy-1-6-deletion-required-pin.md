---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - pin-derivation
  - interest-registry
  - legacy-surface-removal
supersedes: []
related_claims: []
source_lines:
  - 3645-3695
captured_at: 2026-06-12T00:32:21Z
---

# Episode: Legacy {1,6} Deletion Required Pin Re-Derivation from Interest Registry

## Prior State

Open-view pin derivation in ram_eviction.rs referenced thread_view.selected_thread, author_view.selected_author, thread_root_id(), and thread hydration sets — all from the author/thread view state machine that #1100 was deleting.

## Trigger

Opus review of #1100 confirmed worst case: git merges #1096's pin code with zero conflict markers, but the merged result doesn't compile (7 errors at ram_eviction.rs:195-233) — open-view RAM pinning silently breaks because the deleted fields are still referenced.

## Decision

Pin derivation re-derived from lifecycle.registry().iter_active() + shape.matches_event_with_id(...) — the same predicate ingest's matches_active_open_interest uses. Tests migrated to open_interest_sub seam rather than deleted. This makes pins resilient to future view-state deletions.

## Consequences

- Pin source is now the interest registry, not concrete view state structs — architectural invariant strengthened
- Tests cover the generic interest seam instead of the deleted author/thread-specific paths
- claimed_profiles decode cluster promoted to public surface as part of the migration

## Open Tail

*(none)*

## Evidence

- transcript lines 3645-3695

