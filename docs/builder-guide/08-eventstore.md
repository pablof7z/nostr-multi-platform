# 08 — EventStore + insert invariants + GC

**Status: SHIPS** · audience: both · prereqs: [07](07-subscription-planner.md)

The `EventStore` is the one place Nostr events live. It is owned by the
actor, never handed to native code, and has exactly one mutating door:
`insert`. Every supersession, delete, dedup, and expiry rule the spec
promises is enforced *inside that door* — apps never re-implement them.

This section is the operational map: what `insert` does per kind, how
deletes become tombstones, what GC pins, and how a cache miss is (or is
not) authoritative. The byte-level storage layout is [09](09-persistence-lmdb.md).

> The trait: `crates/nmp-core/src/store/events.rs:144-296` (`pub trait
> EventStore`). The shipping in-memory impl: `crates/nmp-core/src/store/mem/`
> (factory at `store/mod.rs:24`, insert invariants at `store/mem/insert.rs`).
> Spec source of truth: `docs/product-spec/subsystems.md:7-55` (§7.1).

---

## The one insert path

`EventStore::insert(event: VerifiedEvent, source: &RelayUrl,
received_at_ms: u64) -> Result<InsertOutcome, StoreError>`
(`store/events.rs:236-241`).

Two structural guards make misuse impossible:

- **`VerifiedEvent`** is the only accepted argument. It is constructed
  solely by `VerifiedEvent::try_from_raw` (Schnorr + id-hash check,
  `store/types/events.rs:141-157`). The unchecked constructor is
  `#[cfg(any(test, feature = "test-support"))]` only
  (`store/types/events.rs:163-166`) — production code cannot fabricate a
  verified event.
- **No index/storage setter is public.** Secondaries, provenance, and
  tombstones are written transactionally by `insert`; there is no
  "add to index" call for an app to reach.

The kernel's relay-frame path proves the contract: `verify_and_persist`
(`kernel/ingest/mod.rs:249-288`) builds a `RawEvent`, calls
`VerifiedEvent::try_from_raw`, and on failure logs + drops — it never
inserts an unverified event.

---

## What happens on insert, by kind

Resolution order is fixed (`store/mem/insert.rs:28-119`); the first
matching rule wins.

| Kind class | Rule | Outcome on success |
|---|---|---|
| Structurally invalid (bad id/pk/sig length) | reject pre-store | `Rejected{Malformed}` |
| Ephemeral (20000–29999) | deliver live, never store | `Ephemeral` |
| NIP-40 `expiration` ≤ now | reject pre-store | `Rejected{ExpiredOnArrival}` |
| Per-id tombstone applies | suppress | `Tombstoned` |
| Addr tombstone (param-repl, `deleted_at ≥ created_at`) | suppress | `Tombstoned` |
| Kind:5 (delete) | apply deletes + store the kind:5 | `Inserted` |
| Replaceable (0, 3, 10000–19999) | supersede by `(pubkey, kind)` | `Inserted`/`Replaced`/`Superseded`/`Duplicate` |
| Param-replaceable (30000–39999) | supersede by `(pubkey, kind, d-tag)` | same four |
| Anything else | normal insert / dedup | `Inserted`/`Duplicate` |

Replaceable winner rule (`store/mem/insert.rs:209-227`): newest
`created_at` wins; tie broken by **lexicographically smallest id**.
Exact-id duplicate is checked *before* supersession so a redelivery
merges provenance rather than churning the index
(`store/mem/insert.rs:182-187`).

Kind ranges are classified by `RawEvent` helpers
(`store/types/events.rs:41-53`): `is_replaceable`, `is_param_replaceable`,
`is_ephemeral`.

---

## `InsertOutcome` variants

`enum InsertOutcome` — `store/types/outcomes.rs:10-26`. Callers that
mutate local projections for replaceable kinds **must** branch on this:
only `Inserted | Replaced` means "this is now canonical" (D4). The
kernel does exactly that for kinds 0/3/10002
(`kernel/ingest/mod.rs:199-238`).

| Variant | Means | Caller action |
|---|---|---|
| `Inserted{id, sources_after}` | fresh row, secondaries written | canonical — project it |
| `Duplicate{id, sources_after}` | known id; provenance merged, primary untouched | no-op for projections |
| `Replaced{new_id, replaced_id}` | newer replaceable won; old row gone | canonical — re-project |
| `Superseded{id, current_id}` | incoming was older; dropped | no-op; current stays |
| `Tombstoned{id, kind5_event_id, origin}` | a tombstone suppressed it | no-op; do not retry |
| `Rejected{id, reason}` | bad sig / delegation / malformed / expired | drop; log |
| `Ephemeral{id}` | 20000–29999; not stored | hand to live consumers only |

`RejectReason` (`store/types/outcomes.rs:28-35`): `BadSignature`,
`BadDelegation(String)`, `Malformed(String)`, `ExpiredOnArrival`.

---

## Tombstone state diagram

A tombstone is a row that *outlives the event it kills* so a later
redelivery cannot resurrect it. `TombstoneRow` /
`TombstoneOrigin` — `store/types/outcomes.rs:39-57`.

```
                 kind:5 by author X, e/a-tag → target authored by X
   (event present)──────────────────────────────────────────► [TOMBSTONED]
        │                                                          │
        │ kind:5 by author X for same target (redelivery)          │ later
        │   merge_tombstone: deleted_at = max, sources ∪           │ redelivery
        ▼                                                          ▼
   primary row removed + provenance removed              insert() pre-check
   tombstone written (origin = Kind5)                    hits tombstone →
                                                         InsertOutcome::Tombstoned

   NIP-40 expiry reaper  ─────────────────────────────► tombstone (origin = NIP40Expiry)
   admin purge / GC      ─────────────────────────────► tombstone (origin = AdminPurge)
```

Author-scoping (`store/mem/insert.rs:57-76`, `269-308`): a kind:5 only
deletes targets **authored by the kind:5's pubkey**. A *foreign*
pre-tombstone (deleter ≠ event author) does **not** block the event —
the row is removed and the insert proceeds (invariant 3,
`store/mem/insert.rs:74-76`). `NIP40Expiry`/`AdminPurge` tombstones apply
unconditionally. Redeliveries max-merge `deleted_at` and union `sources`
(`merge_tombstone`, `store/mem/insert.rs:338-353`) — first-arrived
timestamp is *not* kept.

Address tombstones (`addr_tombstones`, keyed `kind:pubkey:dtag`) handle
a kind:5 `a`-tag arriving before its target param-replaceable
(`store/mem/insert.rs:78-97`, `307`).

---

## Fallback loader contract — the 4 miss types

A store miss is not "does not exist." Whether a miss is *authoritative*
depends on the sync watermark for the relevant `(filter, relay)` pair
(subsystems.md:46-54; `coverage()` at `store/events.rs:254`, see
[09](09-persistence-lmdb.md)). The loader splits by need:

| Miss type | Trigger | Resolution | Authoritative "not found" iff |
|---|---|---|---|
| Pointer-id | `get_by_id` empty | batched+deduped id fetch → relay hints → fallback sources | `Coverage::CompleteAsOf` for a covering filter |
| Address (replaceable coord) | `get_param_replaceable` empty | resolve `(pubkey,kind,d-tag)` same path | as above |
| Tag-value | `scan_by_tags` empty | bounded historical window load; record unknown range | covering watermark `CompleteAsOf` |
| Timeline-window | `scan_by_*_time` short of `limit` | bounded window backfill; record what range is still unknown | covering watermark `CompleteAsOf` |

Rule: a **non-empty** result is never proof a query is complete; only a
covering watermark turns a miss into "not found"
(subsystems.md:52). Custom fallback sources (CDN/mirror) are allowed via
app-kernel extension points, but loaded events still re-enter through the
single verified `insert` path (subsystems.md:54).

---

## GC And Retention

There are two separate bounds:

- **RAM working set.** `Kernel::evict_ram_caches()` bounds the in-memory
  `events` map, profile cache, and contacts cache on the actor's GC tick. This
  loses no durable data; LMDB remains the authoritative event store.
- **Durable store retention.** `gc_step(GcBudget) -> GcReport` does one bounded
  durable maintenance pass: NIP-40 reap, optional explicit finite-retention LRU
  deletion, and tombstone purge. `GcBudget::production()` keeps durable LRU
  deletion disabled (`max_total_events = usize::MAX`) so valid fetched events
  stay queryable offline by default. A finite durable row ceiling must be an
  explicit disk/user policy via `GcBudget::with_durable_event_ceiling`.

When explicit durable retention is enabled, the kernel derives a transient pin
set from live views, component claims, in-flight publishes, and floor-coherent
coverage. The store receives those pins for that one pass; there is no persisted
claim database. `gc_step` is never called from an FFI path — actor-scheduled
only (`gc.md` §3).

---

## Anti-patterns

1. **Bypassing the insert path.** There is no index/storage setter; an
   app that keeps its own event map re-creates every bug §7.1 prevents
   (stale replaceable, phantom note after kind:5).
2. **Mutating events after insert.** `StoredEvent.raw` is
   `Arc<RawEvent>` shared with view payloads
   (`store/types/events.rs:185-189`); treat it as immutable. A new
   version is a new insert that supersedes.
3. **Treating a cache miss as "not on relay" without a watermark.**
   Only `Coverage::CompleteAsOf` makes absence authoritative; miss +
   `Unknown`/`PartialUpTo` means *fetch*, not *empty*.
4. **Manual delete-by-event from app code.** `delete_by_filter` is the
   admin/GC/kind:5 vector and is NMP-internal, not a `nostr::Filter`
   pass-through (`store/types/gc.rs:37-50`). Deletes happen via a
   verified kind:5, not by reaching into storage.
5. **Ignoring `InsertOutcome` for replaceables.** Acting on a
   `Duplicate`/`Superseded` as if canonical re-projects stale state and
   breaks D4 single-writer.

---

See also: [07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md) · [09 — Persistence (LMDB) + watermarks](09-persistence-lmdb.md) · [13 — Sync engine — `nmp-nip77`](13-sync-engine.md) · [21 — The framework-magic contract](21-framework-magic.md)
