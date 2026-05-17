# Framework Magic §C1–§C4 — Replaceable & Delete Invariants

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/product-spec/subsystems.md` §7.1 (the EventStore insert-time invariants table — this chapter references its rows, does not restate them); `docs/design/lmdb-schema.md` (storage backend for M3); `docs/design/lmdb/tests.md` §3 (kind:30023 d-tag corner cases).

This chapter holds four bullets, all of which discharge `docs/product-spec/overview-and-dx.md` §3.3 **bug-extinction #1** ("Stale replaceable event retained in state after a newer one arrives") and `docs/aim.md` §6 **doctrine 4** ("replaceable-event invariants enforced on insert"). The four are split because they cover four distinct kind-class shapes and have four distinct test surfaces.

## C1. Replaceable supersession on insert (kind 0 / 3 / 10000–19999)

**Statement.** Any kind in `{0, 3, 10000..=19999}` arriving at the event store automatically supersedes the prior event with the same `(pubkey, kind)`; the prior event becomes unreachable through the public read path.

**Framework does:** the insert-time supersession at `docs/product-spec/subsystems.md` §7.1 row "Replaceable kinds (0, 3, 10000-19999)". Mechanism: compare `(pubkey, kind)` against the existing entry, keep newest `created_at`, tie-break by lexicographically smallest `id`. The current in-memory kernel partially enforces this: kind:0 (`ingest_profile` at `crates/nmp-core/src/kernel/ingest.rs:166-184`) applies both the `created_at` check and the `id` tie-break correctly; kind:10002 (`ingest_relay_list` at `crates/nmp-core/src/kernel/ingest.rs:218-222`) uses `>=` with no tie-break; kind:3 (`ingest_contacts` at `crates/nmp-core/src/kernel/ingest.rs:206`) uses unconditional overwrite with no monotonicity guard or tie-break. The full canonical rule (strict monotonic + `id` tie-break for all replaceable kinds) lands in M3's LMDB-backed `EventStore` trait (`docs/design/lmdb/trait.md`).

**App writes:** nothing. The app calls `ProfileView::open(pubkey)`; the view's payload reflects the latest kind:0 the store has, with no app-side comparison of `created_at`.

**Failure mode prevented:** §3.3 bug #1. Plus the doctrine-4 footgun: an app caches kind:3 in its own state, fails to re-fetch on UI nav, renders a stale follow list, double-subscribes on the next session.

**Test:** `c1_replaceable_supersedes_on_insert`. The test inserts kind:0 #1 at `created_at=T`, then kind:0 #2 at `T+1` with same pubkey; asserts `ProfileView` payload reflects #2 and that a subsequent insert at `T-1` is rejected (no payload re-emit, no event store change). Tie-break path: two inserts at the same `T` with different ids — the lexicographically-smaller-id event wins, deterministic across runs.

**Milestone owner:** **[PARTIAL]** — kind:0 supersession with tie-break is [DONE] (in-memory kernel, `crates/nmp-core` tests). Kind:3 and kind:10002 supersession lack the canonical tie-break and in kind:3's case lack the monotonicity guard; both graduate to full enforcement in M3's LMDB `EventStore` trait. The C1 test runs against the in-memory kernel from day one but is scoped to kind:0 until M3; a `#[cfg(feature = "m3_lmdb")]` gate in the test enables the full kind:3 and kind:10002 sub-paths on M3 landing.

---

## C2. Parameterized replaceable supersession (kind 30000–39999) by `(pubkey, kind, d-tag)`

**Statement.** Any kind in `{30000..=39999}` is keyed by `(pubkey, kind, d-tag)`, not just `(pubkey, kind)`. Two events with the same kind and pubkey but different `d` tags coexist; two with the same `d` supersede.

**Framework does:** the insert-time rule at `docs/product-spec/subsystems.md` §7.1 row "Parameterized replaceable (30000-39999)". M3 implements this in LMDB via the key encoding at `docs/design/lmdb/keys.md` and the `get_param_replaceable(pk, kind, d_tag)` accessor on the `EventStore` trait (`docs/design/lmdb/trait.md`).

**App writes:** nothing. Long-form (kind:30023) reader views open by `(pubkey, d_tag)` coordinate; the framework resolves to the current event.

**Failure mode prevented:** §3.3 bug #1 for the parameterized case — the most common subtlety being apps that key only on `(pubkey, kind)` and overwrite a kind:30023 with a different `d` tag, losing one of the author's articles.

**Test:** `c2_parameterized_replaceable_supersedes_by_dtag`. Mirrors `docs/design/lmdb/tests.md` line 93: insert two kind:30023 with same `(pubkey, d=foo)`, second newer; assert only the second is read. Insert a third with same kind+pubkey but `d=bar`; assert both `foo` and `bar` are independently retrievable. Insert a kind:30024 with `d=foo`; assert it does not collide.

**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 LMDB"]`; M3 owner removes the ignore as part of the framework-magic delta on M3's exit-gate report. (Note: the M3 LMDB-tests doc already contains the same scenario at the storage layer; C2 promotes it from a storage-layer test to a contract-surface test — the framework-magic test calls through the public view path, not through the EventStore trait directly.)

---

## C3. Kind:5 delete propagation: referenced events removed, tombstone persisted

**Statement.** A signature-verified kind:5 event from author X referencing event ids `[e1, e2, ...]` and/or replaceable coordinates `[a1, a2, ...]` removes any matching events the store holds that are *authored by X*; the deletions persist as tombstones so the same events cannot be re-inserted later.

**Framework does:** §7.1 row "Kind 5 (delete)". Mechanism: after signature verification, scan the referenced `e` and `a` tags, remove matching events *authored by the deleter* (other authors' events with the same id, if any, are untouched — a kind:5 by Alice cannot delete Bob's events), persist a tombstone keyed by event coordinate with a tombstone timestamp = maximum delete `created_at` observed for that target.

**App writes:** nothing. The view payloads recompute (via `ViewModule::on_event_removed` per `docs/design/kernel-substrate.md` §3 lines 141–143) and the deleted note disappears from `TimelineView.items` in the next emit.

**Failure mode prevented:** the cross-cutting "phantom note" bug: a kind:5 lands, the app's UI does nothing, the note still renders, and worse — re-inserts on app restart because the app's local cache predates the delete. The tombstone is the structural answer: even if the original event is re-delivered by another relay, the store refuses to re-insert it.

**Test:** `c3_kind5_delete_removes_referenced_and_tombstones`. The test:

1. Inserts a kind:1 event `e1` by author Alice; asserts it appears in `TimelineView`.
2. Inserts a kind:5 by Alice referencing `e1`; asserts `TimelineView` no longer contains `e1`.
3. Re-inserts `e1` (simulating a later relay redelivery); asserts the store rejects it and the timeline payload does not re-emit.
4. Inserts a kind:5 by Bob referencing `e1`; asserts the tombstone is *not* upgraded (cross-author kind:5 has no effect).
5. Restart the store (M3 path) and re-insert `e1`; assert tombstone is still in force.

**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 kind5 + tombstone persistence"]`. The current kernel (`crates/nmp-core/src/kernel/ingest.rs`) does not handle kind:5 events — they are silently ignored in the `match event.kind` dispatch at line 160. No tombstone logic exists in the in-memory path. All five sub-paths of the test require M3's kind:5 handler and LMDB tombstone subdatabase (`docs/design/lmdb/keys.md`).

---

## C4. NIP-40 expiration auto-removes event at expiry; survives actor restart

**Statement.** An event carrying a NIP-40 `expiration` tag is automatically removed from the store at the expiration timestamp; the schedule survives actor restart.

**Framework does:** §7.1 row "NIP-40 expiration": schedule a timer to remove the event at the expiration timestamp; on actor restart, scan the persisted store and re-schedule any surviving expiration. M3 implements both the timer scheduling and the persistent rescan; the current in-memory kernel does not parse NIP-40 `expiration` tags at all.

**App writes:** nothing. Same `on_event_removed` path as C3.

**Failure mode prevented:** apps shipping their own "is this event expired?" filter, getting it wrong (off-by-one timezone, missing tag parser, not re-checking after restart), and rendering events that should be gone — especially relevant for ephemeral notifications and expiring offers.

**Test:** `c4_nip40_expiration_removes_and_persists_schedule`. The test uses the `SimulatedClock` from `nmp-testing` (`docs/product-spec/subsystems.md` §7.13 line 343):

1. Insert an event with `expiration` tag at clock-now + 60s.
2. Advance clock to +30s; assert event still present.
3. Advance clock to +61s; assert event removed; `TimelineView` payload re-emitted without it.
4. Insert another event with expiration at +120s.
5. Simulate actor restart (drop the actor, instantiate from persisted store); assert the +120s schedule is re-armed by the rescan; advance clock to +130s; assert removal fires.

**Milestone owner:** **[PENDING M3]**. Test checked in as `#[ignore = "pending M3 NIP-40 + expiration persistence"]`. The current kernel does not parse or schedule NIP-40 `expiration` tags — ingested events with expiration are stored without a removal schedule. All five sub-paths require M3's expiration manager and timer-rescan logic; no sub-path is testable against the current in-memory kernel.

---

## Why this chapter is four bullets, not one

The four invariants ride the same insert path but have different observable surfaces, different test trigger shapes, and different milestones own them. Collapsing them would (a) hide which milestone owes which guarantee and (b) make the regression test ambiguous when one breaks while the others pass. The chapter is the granularity the milestone delta protocol needs.

## What this chapter does not cover

- The replaceable rule for kind:10002 (mailboxes) is C1 (it is in `10000..=19999`). C5 (kind:3 auto-tracking) and the M2 `Trigger::Nip65Arrived` are the *reactive* second-order effect; C1 is the *storage* invariant that triggers them.
- Cross-replaceable-kind interactions (e.g., a kind:5 deleting a kind:0): legal but odd. The §7.1 row says kind:5 removes "matching events authored by the deleter" — the replaceable supersession just means the matched event might already be the latest version. No special-case in the contract; the existing rules compose.
- Garbage collection of unreferenced non-pinned events: a separate concern. `docs/product-spec/subsystems.md` §7.1 "GC" + `docs/design/lmdb/gc.md`. Not a contract bullet because the app does not observe GC directly; it observes events appearing and disappearing per the four rules above, and GC just bounds memory.
