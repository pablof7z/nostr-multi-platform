# LMDB sub-design: key encoding

> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).

## 1. LMDB environment layout

One `lmdb::Environment` per app data directory. Sub-databases:

| Sub-db | Owner | Key shape | Value | Notes |
|---|---|---|---|---|
| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
| `idx_author_kind` | NMP | `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | newest-first scans for `(author, kinds[])` |
| `idx_kind_dtag` | NMP | `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` | `event_id[32]` | parameterized replaceable address lookup |
| `idx_etag_time` | NMP | `target_event_id[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | reaction/reply/thread view scans |
| `idx_ptag_time` | NMP | `target_pubkey[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | mentions / notifications |
| `idx_kind_time` | NMP | `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | global-by-kind backfills |
| `idx_expires` | NMP | `expires_at_be[8] ‖ event_id[32]` | empty | NIP-40 reaper |
| `tombstones` | NMP | `target_event_id[32]` | CBOR `TombstoneRow` | persists past delete |
| `provenance` | NMP | `event_id[32]` | CBOR `ProvenanceRow` | per-relay sidecar (master doc §9) |
| `watermarks` | NMP | `filter_hash[32] ‖ relay_url_bytes` | CBOR `WatermarkRow` | M4 NIP-77 sync state |
| `claims_meta` | NMP | `claimer_id_be[8]` | CBOR `Vec<EventId>` | pinned set per ClaimerId; rebuilt on restart from open views |
| `domain_<ns>_data` | NMP, per `DomainModule` | module-defined | module-defined | one sub-db per registered namespace |
| `domain_<ns>_idx_<name>` | NMP, per `DomainModule` index | `index_key ‖ primary_key` | empty | secondary indexes per `DomainIndex` |
| `_meta` | NMP | string namespace | `{ schema_version: u32, opened_with_nmp_version: String }` | migration tracking |

Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.

## 2. Endian + ordering conventions

- All integers in keys are **big-endian** so LMDB's byte-wise comparator matches numeric order.
- `created_at_desc_be = (u64::MAX - created_at).to_be_bytes()` so a forward scan returns newest-first without `MDB_PREV` gymnastics.
- All pubkeys / event ids are fixed-width 32 bytes; the `nostr` crate's `EventId` and `PublicKey` give us byte arrays directly.

## 3. Secondary index details

### 3.1 `idx_author_kind`

Key: `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty value.

Scan recipes:

- *Newest N events by author* — `range(pubkey ‖ 0u32_be ‖ ..)` (kind=0 lower bound) up to `pubkey ‖ u32::MAX_be ‖ ..`, take N.
- *Newest N events by `(author, kind=1)`* — `range(pubkey ‖ 1u32_be ‖ ..)` up to `pubkey ‖ 1u32_be ‖ u64::MAX_be`, take N.
- *All kind:0 for author* — `range(pubkey ‖ 0u32_be ‖ ..)`, take 1 (because the replaceable index ensures only one).

Replaceable supersession (§7.1): on insert of a new kind in [0, 3, 10000–19999], find existing row via this index with `(pubkey, kind)` prefix, compare `created_at`, if incoming wins delete old + write new. Both deletes happen in the same `RwTxn` as the new write so there is no half-state visible to readers.

### 3.2 `idx_kind_dtag` (parameterized replaceable)

Key: `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` → `event_id[32]`.

The d-tag bytes go last so two events with the same `(kind, pubkey)` but different `d` tags don't collide; the explicit length prefix avoids `d="foo"` vs `d="foob"` aliasing under prefix scans. Lookup is exact-key: `get_param_replaceable(pubkey, kind, d_tag)` builds the key and reads.

The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.

### 3.3 `idx_etag_time` and `idx_ptag_time`

Key: `target[32] ‖ created_at_desc_be[8] ‖ event_id[32]` → `kind_be[4]`.

The value holds the kind so a reactions view can filter `(kinds == 7)` during scan without a primary-row fetch per candidate. Bookmark / repost / thread views similarly avoid the `get_by_id` round trip until they need the body.

On insert, the kernel walks the event's `tags`: every `e` tag value goes into `idx_etag_time` and every `p` tag value goes into `idx_ptag_time`. Tag values must be 32-byte hex (validated at insert time); non-conformant tags are silently skipped from indexing (they are still stored in the event body).

### 3.4 `idx_kind_time`

Key: `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty.

Used by *global-by-kind* backfills (e.g. "recent kind:0 across all authors" during diagnostics). Heavy index — populated for **all** kinds by default but the implementation may skip kinds in a configurable deny-list to keep write amplification down (default deny-list: kind:1 if config flag `index_kind1_globally=false`, which it is by default; M2's planner does not need a global kind:1 scan).

### 3.5 `idx_expires`

Key: `expires_at_be[8] ‖ event_id[32]` → empty.

Populated **only** for events that have an `expiration` tag at insert (NIP-40). `gc_step()` opens a read cursor at `expires_at = 0`, walks forward up to the configured budget, and reaps any keys whose `expires_at ≤ now_unix_seconds()`. Each reaped event triggers a tombstone-of-origin `NIP40Expiry` write so re-insertions (from a re-sync) don't resurrect it.

## 4. Tombstones

Key: `target_event_id[32]` → CBOR `TombstoneRow`:

```rust
#[derive(Serialize, Deserialize)]
struct TombstoneRow {
    target_id: [u8; 32],
    origin: TombstoneOrigin,             // Kind5 | NIP40Expiry | AdminPurge
    kind5_event_id: Option<[u8; 32]>,    // None for non-Kind5 origins
    deleter_pubkey: Option<[u8; 32]>,    // None for NIP40Expiry / AdminPurge
    deleted_at: u64,                     // max observed across kind:5 redeliveries
    sources: Vec<String>,                // relay urls that delivered the kind:5
}
```

Insert pre-check: before any new event hits the primary store, `tombstones.contains_key(event.id)` is consulted. A hit yields `InsertOutcome::Tombstoned { target_kind5_id }` and the event is dropped. This is the "later re-insertion is suppressed" behavior of §7.1.

Foreign kind:5 (where the kind:5 author did not author all targets) is **stored** as an ordinary event (so other clients can render the delete intent) but **does not** write a `TombstoneRow` for any of its targets — per §7.1 "foreign kind:5 ignored". The kind:5 event itself goes through the normal insert path including secondaries.

## 5. Watermarks

Key: `filter_hash[32] ‖ relay_url_bytes` — variable-length, exact-key lookups only. `filter_hash` is BLAKE3 of the canonical filter encoding (see `lmdb/watermarks.md` §3 for the canonicalisation algorithm).

Value: CBOR `WatermarkRow` (same shape as the trait type in [`trait.md`](trait.md) §2).

## 6. Provenance

Key: `event_id[32]` → CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }`. On duplicate insert: read, mutate (append or bump `last_seen_ms`), write back. Bounded growth — the kernel caps `sources.len()` at 32 (the 33rd unique relay overwrites the oldest non-primary entry); for nearly all events this is non-binding. The `primary: bool` flag is deterministic: `sources[0]` after sorting by `(first_seen_ms, relay_url)`.

## 7. Domain rows (per `DomainModule`)

For each `DomainModule` with namespace `"foo.bar"`:

- `domain_foo.bar_data` — primary data sub-db. Module owns key + value encoding.
- `domain_foo.bar_idx_<index>` — one sub-db per `DomainIndex` (per `crates/nmp-core/src/substrate/domain.rs:16`). Key = `index_key_fn(data_value) ‖ primary_key`; value = empty. The index is rewritten on every put (delete-old, write-new).

The actor exposes them only via `DomainHandle` (see [`trait.md`](trait.md) §4); modules never see the sub-db handles directly. Module isolation per `kernel-substrate.md` §8 is preserved: the handle factory checks the caller's registered namespace.

## 8. `_meta` sub-database

Key: namespace string (e.g. `"twitter.drafts"`, `"_kernel"`). Value: CBOR `{ schema_version: u32, opened_with_nmp_version: String, last_migration_at_ms: u64 }`. Read at startup by the migration runner; written after every successful migration step.

The reserved `_kernel` namespace tracks the LMDB store's own schema version (currently 1). A bumped `_kernel` version triggers store-wide migrations (e.g. re-encoding all `ProvenanceRow` values when the format changes).

## 9. Worked example: inserting a kind:1 from `pablof7z` arriving from `wss://relay.primal.net`

```
event_id   = a3f1...   (32 bytes)
pubkey     = 0461...   (32 bytes)
kind       = 1
created_at = 1747000000
tags       = [["e","b21c...","","root"], ["p","0488..."]]
```

Inside one `RwTxn`:

1. `tombstones.get(&event_id)` → None ⇒ proceed.
2. `nostr_lmdb.save_event(&event)` → SaveEventStatus::Success.
3. `idx_author_kind.put(0461... ‖ 0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])`.
4. `idx_kind_time.put(0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])` (only if `index_kind1_globally`; default off).
5. For `e:b21c...` → `idx_etag_time.put(b21c... ‖ desc(1747000000) ‖ a3f1..., 1u32_be)`.
6. For `p:0488...` → `idx_ptag_time.put(0488... ‖ desc(1747000000) ‖ a3f1..., 1u32_be)`.
7. `provenance.put(a3f1..., cbor({sources:[{relay:"wss://relay.primal.net", first_seen_ms:T, last_seen_ms:T, primary:true}]}))`.

Total LMDB writes: 1 primary (delegated to upstream) + 3 NMP secondaries + 1 provenance = ~5 page writes for a typical kind:1. Within the 250 µs p99 budget (master doc §12) on iPhone 12 NAND.

A second arrival of the same id from `wss://nos.lol`:

1. `tombstones.get(&a3f1...)` → None.
2. `nostr_lmdb.save_event` → SaveEventStatus::Duplicate (we don't re-process).
3. Skip steps 3–6 (secondaries unchanged).
4. `provenance.get(a3f1...)` → existing row; append `{relay:"wss://nos.lol", first_seen_ms:T2, last_seen_ms:T2, primary:false}`; put back.

One read + one write. Returns `InsertOutcome::Duplicate { sources_after: 2 }`.
