---
type: episode-card
date: 2026-06-18
session: 129d2615-7195-4082-924e-9b96e3f1de8b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/129d2615-7195-4082-924e-9b96e3f1de8b.jsonl
salience: architecture
status: superseded
subjects:
  - relay-kind-index
  - lmdb-provenance
  - store-query
  - eventstore-trait
supersedes: []
related_claims: []
source_lines:
  - 1988-2002
  - 2128-2420
captured_at: 2026-06-18T18:48:44Z
---

# Episode: Design relay-kind provenance index as presence-only, privacy-gated, self-describing projection

## Prior State

NMP had per-event provenance (32-entry LRU) and a V-52 relay-origin reverse index (`nmp-relay-index`: relay→event_id), but no way to query 'which kinds has a relay seen' or 'how many events of kind X on relay Y' without full scans. Provenance indexes were a known gap in the cache/store architecture.

## Trigger

Issue #1518 required queryable derived indexes over provenance for store planning, relay diagnostics, pruning, and cache coverage — specifically relay×kind coverage and counts without full-store scans.

## Decision

Add a `nmp-relay-kind` LMDB sub-database with key schema `(relay_url || 0x00 || kind_4B_BE || event_id_32B)` → empty value (presence-only, no counter that can drift). Privacy gate at write time: kinds 4/13/14/15/1059/1060 never enter the index, plus defense-in-depth read gate. Kind is encoded in the key so the delete path is self-sufficient (no extra event load needed at GC/delete call sites). Index is maintained exclusively inside canonical provenance update transactions (upsert/delete), never independently mutated. Two new `EventStore` trait methods: `relay_kind_coverage(relay_url) → Vec<u32>` and `relay_kind_count(relay_url, kind) → u64`. One-time backfill for cold-restart correctness of pre-#1518 databases.

## Consequences

- `NMP_ADDITIONAL_DBS` bumps from 10→11; `Inner` struct gains `relay_kind: Database<Bytes, Bytes>` field
- Breaking within-crate signature change: `provenance::upsert` and `provenance::delete` gain `relay_kind` + `kind` params, affecting all call sites in insert.rs, delete.rs (×4), gc.rs (×2), insert_kind5.rs
- Presence-only design guarantees the index is a rebuildable derived projection that can never drift from the canonical provenance LRU
- Privacy gate ensures DM-related event kinds physically cannot leak through the relay-kind query surface
- Mem backend must implement full parity (relay_kind HashMap) per existing convention
- #1516 (streaming query_visit) and #1518 share `store_impl.rs` — merge conflict resolution needed on whichever lands second

## Open Tail

- Implementation not yet started — Sonnet agent to execute the plan in isolated worktree after #1522 merges
- Existing V-52 `nmp-relay-index` (`list_events_seen_on`) is out of scope for privacy-gating — could be tightened in a separate issue
- `dump.rs` may want relay-kind in `nmp dump` diagnostics but not required by #1518 scope

## Evidence

- transcript lines 1988-2002
- transcript lines 2128-2420

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-2-design-relay-kind-provenance-index-as.json`](transcripts/2026-06-18-2-design-relay-kind-provenance-index-as.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-2-design-relay-kind-provenance-index-as.json`](transcripts/raw/2026-06-18-2-design-relay-kind-provenance-index-as.json)
