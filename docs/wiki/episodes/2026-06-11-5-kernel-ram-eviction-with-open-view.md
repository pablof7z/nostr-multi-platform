---
type: episode-card
date: 2026-06-11
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: architecture
status: active
subjects:
  - ram-eviction
  - nmp-core
  - pin-sets
  - d8-doctrine
supersedes: []
related_claims: []
source_lines:
  - 3324-3549
captured_at: 2026-06-11T23:31:21Z
---

# Episode: Kernel RAM eviction with open-view pin sets

## Prior State

Three kernel HashMaps (`events`, `profiles`, `seed_contacts`) were insert-only with no eviction, growing without bound — violating D8 ('working-set bounded').

## Trigger

Bug #1088 — unbounded memory growth in long-running sessions.

## Decision

Added `evict_ram_caches()` with HWM bounds (events: 1000, profiles: 2000, seed_contacts: 32). Pin sets protect live working sets: events pinned by timeline ∪ event_claims ∪ open-view working set (thread focused/root/referenced + hydration bookkeeping + requested_* sets + author-matched) ∪ author-view events; profiles pinned by timeline_authors ∪ profile_claims ∪ active_account ∪ open-view authors. Derived once per GC pass before any eviction via `open_view_pins()`.

## Consequences

- Memory growth bounded under D8
- Reviewer caught that open thread/author views had no store fallback — eviction would blank live rows; pin derivation fixed this
- Pin-set predicate is a copy of `thread_items()` membership — drift risk flagged as follow-up (#957 removes the whole stack)
- #1100 (legacy deletion) must re-derive pins from `lifecycle.registry().iter_active()` + `shape.matches_event_with_id`

## Open Tail

- Re-derive pin sets after #957 legacy stack removal
- Extract shared `thread_items()` membership predicate to prevent drift

## Evidence

- transcript lines 3324-3549

