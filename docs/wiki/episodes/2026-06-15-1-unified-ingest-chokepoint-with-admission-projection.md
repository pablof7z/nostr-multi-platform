---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - ingest-chokepoint
  - should-store-event
  - ephemeral-observer
  - pre-kind3-buffer
  - publish-in-flight-pin
supersedes:
  - 2026-06-15-1-unified-event-ingest-chokepoint-replaces-dual
  - 2026-06-15-2-cache-serve-d9-clamp-gap-future
  - 2026-06-15-3-cache-serve-replay-path-bypasses-d9
related_claims: []
source_lines:
  - 84-160
captured_at: 2026-06-15T15:03:56Z
---

# Episode: Unified ingest chokepoint with admission/projection separation

## Prior State

Two separate ingest ladders (local publish via record_local_publish_intent vs relay via handle_event) with different admission and notification paths. Non-replaceables (kind:1/6/7) had no local echo — ghost-post until relay round-trip. should_store_event was a persistence gate (timeline_authors.contains(author)), so non-followed-author events were dropped before store.insert. Ephemeral events fired NIP parsers but NOT app observers (gate was Inserted|Replaced only). pre_kind3_buffer existed as a band-aid for events arriving before kind:3 processing.

## Trigger

Issue #1440 (missing optimistic echo) + code research revealing that the naive fix would cause double store-insert, double observer fire (violating D4 single-fire), and that the admission gate (should_store_event) would silently drop a user's own root notes since users are not in their own follow set.

## Decision

Single unified chokepoint for all event ingestion (ADR-0057). Admission demoted to valid-sig only — should_store_event becomes a read-time projection predicate, not a persistence gate. Observer gate expands to Inserted|Replaced|Ephemeral so app observers receive ephemerals. pre_kind3_buffer deleted (backfill now comes from the complete store on follow). New publish-in-flight GC pin source added so locally-accepted publishes survive GC pressure before relay confirmation. D9 future-date clamp kept in the timeline observer specifically (not in generic notify).

## Consequences

- Complete store enables projection rebuildability after cold restart — no permanent relevance-shaped holes
- Following a new author backfills their prior events from store via cache-serve replay
- Non-social apps (no follow set) can persist and project events via open_interest
- In-flight publishes must be pinned against GC eviction until relay confirmation
- Non-followed cold notes are stored but unpinned → reaped first under LRU pressure
- Relay echo of a locally-published event dedups to Duplicate (single-fire) with relay_count bump

## Open Tail

- Contacts (kind:3) still kernel-owned — PR 3 will extract to capability seam
- Stress harness not yet built to validate all ~30 scenarios
- Codex gap review identified 14 additional HIGH scenarios (addressable RYW/Superseded-silent, kind:5 tombstones, NIP-40 expiry, bad-sig no-poison, pin-release leak, provenance transitions)

## Evidence

- transcript lines 84-160
