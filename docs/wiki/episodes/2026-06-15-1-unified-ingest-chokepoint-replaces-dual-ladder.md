---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - unified-ingest-chokepoint
  - should-store-event-demoted
  - pre-kind3-buffer-deleted
  - read-your-writes-non-replaceable
supersedes:
  - 2026-06-15-1-unified-kind-agnostic-event-ingest-chokepoint
related_claims: []
source_lines:
  - 84-143
  - 2034-2070
  - 2360-2372
captured_at: 2026-06-15T13:54:27Z
---

# Episode: Unified ingest chokepoint replaces dual-ladder persistence/admission system

## Prior State

Two separate ingest arms: kind:1/6 routed through `ingest_timeline_event` (which did its own store.insert, dispatcher fan-out, read-cache append, and observer fire), while kind:7+ went through `verify_and_persist` + separate `notify_event_observers`. `record_local_publish_intent` only handled replaceables (kind:0/3/10002), so non-replaceables had no local echo. `should_store_event` acted as a persistence admission gate — a user not in their own follow set would have their own note silently dropped or parked in `pre_kind3_buffer`. The D4 single-fire invariant depended on discipline across two call sites.

## Trigger

Issue #1440 (ghost-post UX: non-replaceable events invisible until relay echo) and root-cause issue #1442 (`should_store_event` was a persistence gate masquerading as an admission/view gate). Research revealed the issue's suggested fix would cause double store-insert, double dispatcher fan-out, and double observer fire — and that a naive reuse of `ingest_timeline_event` would silently fail for fresh root notes because the author isn't in their own follow set.

## Decision

Replace the dual ingest path with a single `ingest_accepted_event(IngestSource, event)` chokepoint. All events route through `verify_and_persist` (which now owns `notify_event_observers` gated on `Inserted|Replaced|Ephemeral`, enforcing D4 single-fire by architecture). `should_store_event` demoted from persistence gate to a read-time timeline-view predicate (a `false` only skips projection, the event is already stored). `pre_kind3_buffer` deleted (backfill via cache-serve). `local_publish_intent.rs` deleted. Both relay and local-publish sources route through the same chokepoint. Publish-in-flight pin added to `derive_store_pin_set` to prevent GC of locally-published events awaiting relay confirmation.

## Consequences

- Read-your-writes works for all event kinds (kind:1/6/7 now appear locally before relay echo)
- D4 single-fire invariant enforced by architecture (one call site in `verify_and_persist`) not discipline
- Non-followed author's events persist in store but don't project into timeline view — persistence decoupled from projection relevance
- Duplicate events still bump `relay_count` but are projection-silent (no observer fire on Duplicate)
- Ephemeral events reach parsers and observers but are not stored (observer gate on `Ephemeral` outcome)
- The `should_store_event` function carries a 'do not reintroduce a store.insert gate' warning comment
- Downstream consumer apps (tenex-off, podcast-player, hl) need upgrade + NMP version cut (outside monorepo scope)

## Open Tail

- PR 2 (profiles → ProfileLookup capability seam) and PR 3 (contacts → parser + effect seam) depend on chokepoint and are queued
- Workstream F (doctrine gates banning `store.insert`/`notify_event_observers` outside chokepoint) can now land to prevent dual-ladder regrowth
- NMP consumer app version cut is a separate release step

## Evidence

- transcript lines 84-143
- transcript lines 2034-2070
- transcript lines 2360-2372
