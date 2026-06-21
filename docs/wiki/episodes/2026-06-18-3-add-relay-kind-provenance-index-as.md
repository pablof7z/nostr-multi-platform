---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: active
subjects:
  - relay-kind-index
  - provenance
  - lmdb-store-backend
supersedes:
  - 2026-06-18-2-design-relay-kind-provenance-index-as
related_claims: []
source_lines:
  - 2700-2712
  - 3055-3059
captured_at: 2026-06-18T19:35:30Z
---

# Episode: Add relay×kind provenance index as D4-compliant sub-database

## Prior State

No secondary index existed for relay×kind queries. Relay-filtered kind lookups required scanning all events or using broader indexes, limiting cache-serve efficiency.

## Trigger

Issue #1518 required a queryable relay provenance index to support kind-scoped relay queries, derived from provenance data and written inside the single-writer insert transaction.

## Decision

Add nmp-relay-kind sub-database: key = relay_url || 0x00 || kind_BE4 || event_id_32, value = empty (presence-only). Privacy gate: kinds 4, 13, 14, 15, 1059, 1060 never enter the index. Written only in provenance::upsert/delete (D4 single-writer-per-fact). NMP_ADDITIONAL_DBS bumped in open.rs.

## Consequences

- Enables efficient relay×kind queries without scanning the full event table
- D4 doctrine preserved: index is derived from provenance, written in the same RwTxn as provenance upsert
- Privacy-sensitive DM kinds excluded from the index at the storage layer
- MemEventStore gained matching relay_kind_add/relay_kind_remove_id for parity

## Open Tail

- Cache-serve can now use this index for relay-scoped interest resolution (#1520 will wire wakeups through it)

## Evidence

- transcript lines 2700-2712
- transcript lines 3055-3059

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-3-add-relay-kind-provenance-index-as.json`](transcripts/2026-06-18-3-add-relay-kind-provenance-index-as.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-3-add-relay-kind-provenance-index-as.json`](transcripts/raw/2026-06-18-3-add-relay-kind-provenance-index-as.json)
