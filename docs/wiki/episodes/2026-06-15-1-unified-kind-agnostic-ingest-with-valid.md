---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-chokepoint-unification
  - admission-gate-removal
  - should-store-event-demotion
  - storage-bound-model
supersedes:
  - 2026-06-15-1-ingest-three-layer-separation-admission-persistence
related_claims: []
source_lines:
  - 17-19
  - 126-153
  - 964-998
  - 1028-1052
  - 1164-1188
  - 1241-1266
  - 1378-1418
  - 1427-1428
captured_at: 2026-06-15T10:36:49Z
---

# Episode: Unified kind-agnostic ingest with valid-sig-only admission

## Prior State

Dual ingest paths: replaceable events (kind:0/3/10002) get optimistic local echo via record_local_publish_intent, while non-replaceables (kind:1/6/7) silently return at record_local_replaceable_intent lines 59-62, producing the ghost-post UX (issue #1440). should_store_event acts as both store-admission gate AND timeline-read filter, dropping events whose author isn't in the user's follow set — including the user's own notes — creating relevance-shaped holes in the authoritative store (issue #1442). Kind:0/3/7/10002 already bypass this gate via verify_and_persist (valid-sig-only admission), making the gate incoherent.

## Trigger

Issues #1440 and #1442 traced to the same root cause: persistence entangled with relevance. Code investigation revealed (1) a user's own note can be dropped because they aren't in their own follow set (should_store_event timeline_authors.contains check), (2) the admission gate is incoherent across kinds, and (3) the suggested fix in the issue would double-insert and double-fire observers. User directive: reject acquisition-match admission checks as needless complexity (signatures prevent forgery, projections filter at read-time, a relay we chose to connect to isn't a meaningful threat model).

## Decision

Admission collapses to valid-signature-only (source-agnostic, kind-agnostic). Relevance is a read-time concern only. A unified chokepoint ingest_accepted_event(source, event) replaces the dual local/relay paths. should_store_event is demoted from store admission gate to a timeline-view predicate. Pin-aware LRU eviction (HOT_EVENT_CEILING=10k, 60s GC tick) replaces the relevance gate as the single storage bound. The store is understood as a bounded local cache, not an infinite log; persist-everything means admission is unconditionally kind-agnostic, eviction is the bound.

## Consequences

- Read-your-writes for ALL event kinds — local publish hits the same chokepoint as relay ingest, observers fire once (D4 invariant preserved)
- Complete store with no relevance-shaped holes — cache-serve and offline are sound, NIP projections rebuildable from store, cross-session dedup floor restored
- pre_kind3_buffer deleted — it only existed to park events the entanglement would drop
- should_store_event survives only as the timeline read-view filter, not store admission
- Workstream B (acquisition one-door) decoupled from PR 1 — no longer a correctness prerequisite, just DRY/maintainability
- The prior 'gc_step never called in production' finding is stale — GC is wired (actor/mod.rs:2319-2334)
- #1443 filed as follow-up: research unbounded durable store (LMDB is memory-mapped/disk-bound, likely the 10k ceiling conflates durable footprint with RAM working set)

## Open Tail

- Ephemeral events: user corrected 'non-ephemeral' as the admission gate — apps must receive ephemeral events. Investigation into how ephemerals flow (relay path fires observers then drops from store) was in progress at session end; ADR must pin whether ephemerals enter the chokepoint with a no-persist flag or bypass it
- derive_store_gc_inputs disables LRU when the floor-coherent pin scan truncates (ram_eviction.rs:309-316) — must-verify in PR 1's test plan that this doesn't regress into unbounded growth under persist-everything
- Profiles/contacts parser migration deferred to PR 2/3 — ingest_contacts drives planner/lifecycle side effects (CompileTrigger, sync_follow_feed_interests, timeline_authors rebuild) that an IngestParser structurally cannot reach

## Evidence

- transcript lines 17-19
- transcript lines 126-153
- transcript lines 964-998
- transcript lines 1028-1052
- transcript lines 1164-1188
- transcript lines 1241-1266
- transcript lines 1378-1418
- transcript lines 1427-1428
