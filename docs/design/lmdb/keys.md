# LMDB Sub-Design: Key Encoding

This page names the active NMP-managed key families. Upstream `nostr-lmdb`
owns its own primary event rows and internal indexes.

## Active NMP Keys

| Sub-db | Key | Value | Purpose |
|---|---|---|---|
| `idx_author_kind` | `pubkey || kind || created_at_desc || event_id` | empty | newest-first author/kind scans |
| `idx_kind_dtag` | `kind || pubkey || dtag_len || dtag` | `event_id` | parameterized replaceable exact lookup |
| `idx_kind_dtag_time` | `kind || dtag_len || dtag || created_at_desc || event_id` | empty | newest-first cross-author d-tag scans |
| `idx_etag_time` / `idx_ptag_time` | `target || created_at_desc || event_id` | `kind` | thread/reaction/mention scans |
| `idx_kind_time` | `kind || created_at_desc || event_id` | empty | global-by-kind scans |
| `idx_expires` | `expires_at || event_id` | empty | NIP-40 expiry GC |
| `tombstones` | `target_event_id` | `TombstoneRow` | event-id delete suppression |
| `tombstones_addr` | `pubkey || kind || dtag_len || dtag` | `TombstoneRow` | address delete suppression |
| `provenance` | `event_id` | `ProvenanceRow` | per-relay source tracking |
| `nmp-coverage` | store coverage key | `CoverageRow` | per-filter/relay completed coverage |

All integer fields are big-endian. Descending time keys use
`u64::MAX - created_at` so a forward LMDB scan returns newest first.

## Atomicity

Secondaries, provenance, tombstones, and coverage writes are made through the
store helpers that own their transaction boundaries. A successful insert cannot
leave primary data and NMP-owned sidecars out of sync.
