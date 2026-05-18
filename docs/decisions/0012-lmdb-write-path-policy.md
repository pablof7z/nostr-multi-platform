# ADR 0012: LMDB write-path policy — MemEventStore canonical, fork compensates

**Date:** 2026-05-18
**Status:** accepted
**Resolves:** PD-028 (T136b write-path policy)
**Depends on:** ADR-0011 (env sharing), PD-026 (Option B: pinned local fork)
**Related:** D4 (single writer per fact), D2 (typed outcomes), D6 (no panics across FFI), D8 (bounded working set)

## Context

T136b lands an `LmdbEventStore` that backs the same `EventStore` trait as the in-memory `MemEventStore`. Two backends must produce **identical observable behavior** for the same input — otherwise the kernel's outcome-based logic (`Inserted | Replaced | DupNoOp | Superseded | Rejected`) breaks the moment the storage backend is swapped under it.

Both backends already exist:

- `crates/nmp-core/src/store/mem/insert.rs` — canonical NMP-side pipeline operating on `RawEvent` / `VerifiedEvent`.
- `crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs::save_event_with_txn` (line 445) — the fork's write primitive operating on `nostr::Event`.

The pipelines overlap but differ in semantics in non-trivial ways. This ADR declares which side is canonical, enumerates the divergences, and specifies how the adapter compensates.

## Mem path (canonical)

`MemEventStore::insert` (`crates/nmp-core/src/store/mem/insert.rs:22-120`):

1. **Structural validation** (`:29`) — non-empty hex id/pubkey/sig → otherwise `Rejected(Malformed)`.
2. **Ephemeral kinds** (`:37`) — 20000–29999 short-circuit to `Ephemeral { id }`; not stored.
3. **NIP-40 expiration on arrival** (`:42-50`) — if `expiration` tag's value ≤ `received_at_ms/1000` → `Rejected(ExpiredOnArrival)`.
4. **Per-id tombstone check** (`:58-76`) — if a tombstone exists for this id:
   - `TombstoneOrigin::Kind5` with `deleter_pubkey == event.pubkey` → `Tombstoned`.
   - `NIP40Expiry` / `AdminPurge` → `Tombstoned` unconditionally.
   - Foreign kind:5 tombstone (deleter ≠ author): **remove the tombstone**, allow insert (invariant 3 — foreign pre-tombstones must not block the author's own event).
5. **Address tombstone check for param-replaceable** (`:79-97`) — `kind:pubkey:dtag` key; if `deleted_at >= event.created_at` → `Tombstoned`.
6. **Kind:5 self-delete handling** (`:100-102`, helper `:257-315`):
   - Walk `e`-tag targets, **author-only** (`existing.pubkey == kind5_pubkey`): remove primary + provenance, write tombstone via `merge_tombstone` (max-merge `deleted_at`, union sources).
   - Walk `a`-tag targets, author-only, removes all events ≤ `kind5.created_at` for that coordinate; writes per-id and address tombstones.
   - Stores the kind:5 event itself.
7. **Replaceable supersession** (`:105-108`, helper `:171-234`): key = `(pk, kind, None)`. P2 fix: exact-id duplicate check **before** supersession comparison; max by `(created_at desc, id asc)`; tie-break by lower id wins.
8. **Param-replaceable supersession** (`:111-116`): same shape, key = `(pk, kind, Some(d_tag))`.
9. **Normal insert / duplicate** (`:119`, helper `:236-255`): hex-id lookup; existing → upsert provenance, `Duplicate`. New → store + upsert provenance, `Inserted`.

Provenance helper (`mem/mod.rs:149-187`) — every Inserted/Duplicate/Replaced upserts:
- LRU cap of 32 entries (`MAX_PROVENANCE_ENTRIES`).
- Update first/last seen on existing relay entry.
- At capacity, overwrite oldest non-primary entry.
- Sort by `(first_seen_ms asc, relay_url asc)`; mark index 0 as `primary`.

## Fork path (delta — what `save_event_with_txn` does)

`Lmdb::save_event_with_txn` (`crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs:445-513`):

| Step | Fork behavior | Divergence from Mem |
|------|---------------|---------------------|
| Ephemeral check (`:451`) | `Rejected(Ephemeral)` | Mem returns `Ephemeral{id}` — **different outcome type**. |
| Duplicate check (`:456`) | `Rejected(Duplicate)` | Mem returns `Duplicate{id, sources_after}` with provenance count — **fork lacks provenance**. |
| `is_deleted` check (`:461`) | `Rejected(Deleted)` if id is in `deleted_ids` sub-db | Fork's `deleted_ids` is a presence-bit set (`Database<Bytes, Unit>`) — **no `deleted_at`, no kind5_id, no origin, no deleter_pubkey, no sources**. Mem's `TombstoneRow` carries all of that. |
| Coordinate-tombstone check (`:467-473`) | If `deleted_coordinates[coord] >= event.created_at` → `Rejected(Deleted)` | Fork tracks coordinate→timestamp; Mem tracks `kind:pk:dtag → TombstoneRow` with full metadata. |
| Replaceable replacement (`:476-485`) | `find_replaceable_event` then either `Rejected(Replaced)` (incoming older) or `remove_replaceable` + continue (incoming wins) | Mem returns `Superseded{id, current_id}` (older) or `Replaced{new_id, replaced_id}` (newer). **Fork loses both ids in outcome.** |
| Addressable replacement (`:488-500`) | Same shape via `find_addressable_event` / `remove_addressable` | Same outcome-id loss. |
| Deletion-event handling (`:503-508`) | `handle_deletion_event` walks `e`-tags + coordinates, **mismatched-author → `Rejected(InvalidDelete)`** for the whole event (line 1299: any foreign target rejects entire kind:5). | Mem **silently skips foreign targets** and stores the kind:5 anyway (`mem/insert.rs:271 continue`). Different outcome for same input. |
| Per-id tombstone metadata | **Not written** — only `deleted_ids` presence bit per target. | Mem writes full `TombstoneRow` per target. |
| Foreign pre-tombstone removal | **Not done** — `is_deleted` rejects all matching ids unconditionally. | Mem removes foreign kind:5 pre-tombstones (`mem/insert.rs:74-76`). |
| NIP-40 `ExpiredOnArrival` | **Not checked** — fork has no `expiration` semantics on insert. | Mem rejects expired events at the door. |
| Provenance | **None** — concept does not exist upstream. | Mem maintains the 32-entry LRU per id. |
| Final insert (`:510`) | `store(txn, fbb, event)` — encodes flatbuffer, writes 7 secondary indexes. | Mem inserts into `HashMap`. |

## Decision

**`MemEventStore` is canonical.** The LMDB adapter (`store/lmdb/`) implements the same observable behavior by **pre- and post-compensating** around `save_event_with_txn`:

1. **Adapter does Mem's pre-checks BEFORE calling the fork** (inside the same `RwTxn` for atomicity):
   - Structural validation, ephemeral short-circuit, NIP-40 expiration check.
   - Per-id tombstone lookup against the **NMP-side tombstone sub-db** (not the fork's `deleted_ids`).
   - Address tombstone lookup against the **NMP-side addr-tombstone sub-db** (not the fork's `deleted_coordinates`).
   - Foreign kind:5 pre-tombstone removal.
   - For replaceable / addressable: pre-query the existing event id via `find_replaceable_event` / `find_addressable_event` so the post-step can synthesize `Replaced { new_id, replaced_id }`.

2. **Adapter calls `save_event_with_txn`** (transactional event write + index updates).

3. **Adapter POST-compensates based on fork return value**:
   - `Success` + had-existing → `Replaced { new_id, replaced_id }`; else → `Inserted { id, sources_after }`.
   - `Rejected(Duplicate)` → `Duplicate { id, sources_after }` (provenance upsert in NMP sub-db gives count).
   - `Rejected(Replaced)` → `Superseded { id, current_id }` (current_id from the pre-query).
   - `Rejected(Deleted)` → `Tombstoned { id, kind5_event_id, origin }` (metadata from NMP tombstone sub-db).
   - `Rejected(InvalidDelete)` → adapter must **not** allow this outcome to surface — it would diverge from Mem's silently-skip-foreign-target behavior. Adapter pre-filters the kind:5 event's tags to remove foreign-author targets before passing to the fork. If after filtering no valid targets remain but `e`/`a` tags were present, adapter writes only the kind:5 event itself and never invokes the fork's deletion path with the foreign tags.
   - `Rejected(Ephemeral)` → never reached (adapter pre-shortcircuits).

4. **Adapter writes NMP-side metadata in the same `RwTxn`**:
   - Provenance upsert (32-entry LRU, primary flag) on every Inserted/Duplicate/Replaced.
   - Tombstone write on every kind:5 delete (per `e`-tag target AND address-level for `a`-tags) with `max-merge` of `deleted_at` and union of `sources`. **Unconditional** — even if the target event was never stored, the tombstone is recorded so future arrivals are blocked correctly.
   - NIP-40 tombstone write on `ExpiredOnArrival` rejection (origin = `NIP40Expiry`).

5. **Read-side compensation**: `tombstones_for` reads from the NMP-side tombstone sub-db, **not** from the fork's `deleted_ids`. The fork's `is_deleted` set is kept in sync (every NMP tombstone write also marks the corresponding fork entry) so that `save_event_with_txn`'s own `is_deleted` check still fires.

## Consequences

What the adapter must do that `save_event_with_txn` does **not**:

- **Maintain a provenance LRU sub-db** (`event_id → Vec<ProvenanceEntry>`), shared txn with event write.
- **Maintain a richer tombstone sub-db** (`target_id → TombstoneRow{deleter_pubkey, deleted_at, sources, origin, kind5_event_id}`) and an address-tombstone sub-db (`kind:pk:dtag → TombstoneRow`). Fork's `deleted_ids` / `deleted_coordinates` are kept in lockstep but are no longer authoritative for metadata.
- **Pre-query existing id** for replaceable / addressable kinds so `Replaced.replaced_id` and `Superseded.current_id` can be reported.
- **Pre-filter kind:5 `e`/`a` tags** to drop foreign-author targets before invoking `save_event_with_txn`, preserving Mem's "silently skip foreign target" semantics.
- **Remove foreign pre-tombstone** (deleter ≠ event.pubkey) before allowing insert.
- **Enforce NIP-40 expiration on arrival** with `Rejected(ExpiredOnArrival)`.
- **Convert `RawEvent` ↔ `nostr::Event`** at the adapter boundary. The conversion uses a JSON round-trip (`serde_json::to_string(&raw_event)` → `nostr::Event::from_json`). This is the only place in the codebase that performs this conversion on the hot path; a future optimization can cache the parsed `nostr::Event` inside `VerifiedEvent` (which already does the same parse during `try_from_raw`).
- **Provide 8 NMP-side sub-dbs** beyond the fork's 11: `nmp-provenance`, `nmp-tombstones`, `nmp-addr-tombstones`, `nmp-watermarks`, `nmp-claims-budget`, `nmp-claims`, `nmp-domain-versions`, `nmp-domain-data` (single sub-db, namespace-prefixed keys for clean separation without exhausting `max_dbs`). `Lmdb::open_env(..., additional_dbs = 8)`.

Read methods that the fork's primitives **do not** match cleanly are wrapped in adapter logic; the canonical scan semantics (newest-first, `(created_at desc, id asc)`) are guaranteed by the fork's `BTreeSet`-backed `query` and by ordered keys in the `*_iter` helpers.

## Alternatives rejected

- **Delegate fully to fork's `save_event_with_txn` and accept outcome drift.** Rejected: this breaks D4 single-writer-per-fact at the outcome level. Kernel logic that branches on `Inserted | Replaced | DupNoOp | Superseded | Rejected` would behave differently against LMDB vs Mem, defeating the purpose of having a shared trait.

- **Mirror Mem's entire pipeline in raw heed, bypassing `save_event_with_txn`.** Rejected: re-implements all 7 secondary indexes + NIP-09 / replaceable / addressable handling on top of the fork's existing primitives. Doubles the maintenance surface and means every upstream re-sync also has to re-validate NMP's hand-rolled pipeline. The compensate-around-the-primitive approach keeps the upstream-divergent surface minimal (8 sub-dbs + 5 pre/post hooks) and the re-sync delta small.

- **Upstream PR to extend `save_event_with_txn` with NMP semantics.** Rejected for this milestone: would require upstream to accept provenance + extended tombstone metadata, neither of which fits their data model. PD-026's "one release cycle in our fork" stance applies.
