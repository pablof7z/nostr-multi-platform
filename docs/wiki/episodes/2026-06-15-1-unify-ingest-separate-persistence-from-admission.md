---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - kernel-ingest-chokepoint
  - should-store-event-admission
  - local-publish-intent
  - timeline-read-cache
  - event-store-completeness
supersedes:
  - 2026-06-15-1-ingest-persistence-relevance-entanglement-is-architectural
related_claims: []
source_lines:
  - 124-165
  - 385-423
  - 427-473
  - 556-580
  - 588-651
  - 887-930
captured_at: 2026-06-15T09:00:57Z
---

# Episode: Unify ingest: separate persistence from admission from projection

## Prior State

Two parallel per-kind ingest ladders exist — handle_event (relay) and record_local_publish_intent (local) — with persistence gated by relevance in should_store_event. Kind:1/6 are the ONLY kinds where store.insert is gated by timeline_authors.contains (follow set); all other kinds persist unconditionally via verify_and_persist. The timeline read-cache (self.events/self.timeline) is hard-wired into ingest_timeline_event instead of being an observer. Self-authored notes fail admission because users are not in their own follow set.

## Trigger

Code investigation of #1440 confirmed the root cause: ingest_timeline_event (timeline.rs:18) fuses authoritative persistence (store.insert at :102) with in-memory read-cache projection (self.events/self.timeline at :250/289), and the projection's relevance gate short-circuits persistence. Only kind:1/6 hit this gate; kind:0/3/7/10002 persist unconditionally — a kind-asymmetric hole in the authoritative store.

## Decision

Three-layer architecture replaces the dual-ladder design: (1) Admission — per-source trust boundary (relay=subscribed, local=unconditional, replay=already admitted); (2) One kind-agnostic chokepoint (verify_and_persist with notify_event_observers pulled inside, gated on Inserted|Replaced); (3) Projections as registered observers where should_store_event becomes a read-time relevance predicate consulted only by the timeline-cache observer. Delete local_publish_intent.rs; route publish_engine through the chokepoint with local://publish provenance. Demote self.profiles, self.seed_contacts, and timeline read-cache to IngestParsers/observers. Chokepoint sits below handle_event (not routing local through it) per codex refinement — handle_event retains relay bookkeeping, then calls the shared chokepoint.

## Consequences

- Collapses #1440 (ghost posts), #1442 (relevance-shaped holes), cache-serve/offline breakage, projection rebuildability violation, and D0 social-assumption leak — they were always one bug
- Store becomes a complete GC-bounded log of every validly-signed non-ephemeral event; storage bounded by GC/watermark alone, not ad-hoc relevance
- Read-your-writes for all kinds falls out for free — local publishes admitted unconditionally, no per-kind arm needed
- pre_kind3_buffer deleted — only existed to park events the admission/persistence entanglement would have dropped
- Dual-ladder hand-sync drift eliminated structurally — one ingest path means relay echo of local publish dedups to Duplicate, observers fire once
- EventIngestDispatcher migration (crate-boundaries.md §4.2 steps) completed — profile/contacts/timeline caches become registered parsers
- Issue #1442 filed as architectural defect cross-referencing #1440 as the local-publish half of the same root cause
- Delivery planned as single atomic PR per owner directive — steps are internally coupled, intermediate merge leaves kernel half-migrated

## Open Tail

- Whether self.profiles/self.seed_contacts have synchronous kernel readers that need capability-trait read seams before demotion to observers
- Store-size and GC impact of persisting all events (no relevance gate means larger store; GC/watermark must absorb)
- Gift-wrap kind:1059 exclusion from local echo — handle via parser registry, not a kind literal, per D0
- ADR still to be drafted before implementation

## Evidence

- transcript lines 124-165
- transcript lines 385-423
- transcript lines 427-473
- transcript lines 556-580
- transcript lines 588-651
- transcript lines 887-930
