# Decision: `nostrdb-rs` for the M3 LMDB EventStore backend

> **Status:** Accepted (decision)
> **Date:** 2026-05-18
> **Resolves:** the deferred question in `docs/design/nostrdb-notedeck-lessons.md` §2.5 / §5
> **Relates to:** ADR-0072 (LMDB env sharing), doctrine D4 + D8
> **Decision:** **Reject `nostrdb-rs`; keep the hand-rolled `crates/nmp-core/src/store/lmdb.rs` path (targeting `nostr-lmdb` per ADR-0072).**

## 0. Scope clarification (read first)

Two different projects are easy to conflate:

- **`nostrdb-rs`** — Damus's Rust binding to the C `nostrdb` (strfry-derived, LMDB-backed). *This doc's subject.*
- **`nostr-lmdb`** — the rust-nostr workspace crate. *ADR-0072's subject; what the hand-rolled `LmdbEventStore` already targets.*

`nostrdb-notedeck-lessons.md` §2.5 gave a *preliminary* lean toward `nostrdb-rs` and explicitly said "revisit at the start of M3." This document is that revisit and records the decision. Rejecting `nostrdb-rs` does not contradict ADR-0072 — ADR-0072 (env-sharing with `nostr-lmdb`) stands unchanged.

## 1. `nostrdb-rs` actual API + storage model (evidence)

From docs.rs/nostrdb (v0.4.0) and the C `nostrdb` README + Damus dev thread:

- **Construction:** `Ndb::new(db_dir: &str, config: &Config) -> Result<Self>`. The LMDB environment is created **internally by the C library** from the path + `Config`. mapsize auto-halves on failure. **No environment-injection constructor exists.**
- **Ingest:** `process_event(&self, json: &str) -> Result<()>`, `process_client_event`, `process_event_with(json, IngestMetadata)`. Documented as: *"returns immediately and doesn't provide any information on if ingestion was successful."* Fire-and-forget JSON in.
- **Architecture:** strfry-derived **share-nothing**: a dedicated **ingester thread verifies signatures** then transfers ownership to a **single writer thread** ("LMDB allows multithreaded reads… but only a single-threaded writer" — Damus dev thread). Mutation is owned by nostrdb's own threads.
- **Query:** `query(&self, txn: &Transaction, filters: &[Filter], max_results: i32) -> Result<Vec<QueryResult>>` returns a **materialized `Vec`**. Current upstream also exposes `fold` / `try_fold` on query iterators, so early-stopping scans are expressible.
- **Subscriptions:** `subscribe(&[Filter]) -> Subscription`, `poll_for_notes`, `wait_for_notes` — low-level poll/wake.
- **Storage:** custom flatbuffer-like packed note layout mmap'd in LMDB; zero-copy reads; a per-note mutable metadata table (`get_note_metadata`, `NoteMetadataBuf`).
- **Mutation surface:** **no insert/write/update/delete methods in the Rust binding.** No NIP-09 delete entry point. Replaceable / kind:5 semantics are applied *internally by nostrdb on its own terms* during ingest, not steerable by the caller.

## 2. Can it host NMP's insert invariants?

NMP's `EventStore::insert` (`store/mem/insert.rs`) is a single ordered policy pipeline returning a typed `InsertOutcome`. Point-by-point:

| Invariant | Hostable in `nostrdb-rs`? |
|---|---|
| Replaceable / param-replaceable supersession (`Replaced`/`Superseded` outcomes, created_at + id tiebreak) | nostrdb does *a* version internally, but the outcome is **not observable or steerable**; `process_event` returns `()`. NMP cannot emit `InsertOutcome` or drive the reverse index from it. |
| kind:5 tombstones with **self-delete-only** + **foreign pre-tombstone removal** (insert.rs steps 4–6, 257–315) | No delete/tombstone API. nostrdb's internal NIP-09 policy is fixed; NMP's foreign-pre-tombstone and deleter-pubkey rules cannot be injected. |
| NIP-40 expiry-on-arrival rejection (`ExpiredOnArrival`) | No pre-ingest hook returning a reject reason; would have to be a filter wrapper above the store. |
| Provenance max-merge (per-id source set, `sources_after`) | nostrdb has no provenance concept; would live entirely in an NMP sidecar **with no shared transaction**. |
| Claim-based GC (`register_view_cover`/`claim`/`release`/`gc_step`) | nostrdb owns its own LRU/retention; no claim API. NMP's working-set pinning cannot be expressed. |
| Single typed `InsertOutcome` for the actor + ADR-0072 diagnostics | Structurally impossible: ingest is `Result<()>` fire-and-forget. |

**Conclusion:** every NMP insert invariant would have to be re-implemented *above* `nostrdb-rs`, querying after the fact to infer what happened — with **no atomicity** between nostrdb's write and NMP's sidecars such as provenance, tombstones, and coverage. This is strictly worse than the hand-rolled path and reintroduces the partial-write bug class ADR-0072 exists to prevent.

## 3. D4 analysis (single writer per fact)

**Violated.** D4 requires NMP to be the one writer per fact, with caches deriving mechanically. `nostrdb-rs` owns its **own ingester + writer threads** and applies its own replaceable/deletion policy inside them. `process_event(json)` is the only mutation path and it is fire-and-forget: NMP cannot interpose, cannot serialize against its own secondaries, cannot produce the canonical `InsertOutcome`. The "single source of truth" would split across two independently-mutating engines. Irreconcilable with D4.

## 4. D8 analysis (reactivity contract)

**Violated on the load-bearing axis.** The D8 incompatibility is the
**composite reverse index**: D8 requires NMP to maintain a reverse index that,
on each insert, names exactly *which interested views must be woken*. That
index is NMP-owned and is the mechanism behind the reactivity contract.
nostrdb's wake model is a **flat filter-poll subscription** (`subscribe(&[Filter])`
and `poll_for_notes` / `wait_for_notes`): a fixed set of filters that the caller
polls for "you have new notes." It cannot express "this insert touches these
named views and only these," cannot carry NMP's view-identity into the wake, and
cannot be driven from NMP's own insert pipeline (which nostrdb does not expose
— see §2/§3). The reverse-index-naming-interested-views requirement is entirely
NMP's and has no expression in nostrdb's subscription model.

## 5. ADR-0072 compatibility (env ownership)

**Violated outright.** ADR-0072's accepted decision is *"NMP owns the `lmdb::Environment` and injects it into [the store crate]"* so that `insert()` commits event + provenance + coverage + tombstones in **one `RwTxn`**. `Ndb::new(db_dir, &Config)` creates the env internally with **no injection seam** (no `with_env`, no exposed txn-scoped write). The two-environment fallback in ADR-0072 §"Two-phase-write fallback" was rejected as the primary design due to write-amplification and recovery-window ambiguity — and it would be forced here.

## 5b. License checkpoint (blocks code-level borrowing)

There is an **unresolved licensing ambiguity** that independently blocks any code-level reuse:

- `nostrdb-rs`'s `Cargo.toml` advertises **`license = "GPL-3.0-or-later"`** in its crate metadata.
- The upstream C `nostrdb` repository appears to be **BSD-licensed**.

These two facts are in tension and have **not** been reconciled with upstream. Until the discrepancy is resolved in writing:

- **Code-level borrowing — copying, adapting, or vendoring source from `nostrdb` or `nostrdb-rs` into NMP — is blocked.** GPL-3.0-or-later is incompatible with NMP's licensing posture, and we cannot rely on the C-repo BSD notice to override the crate's published GPL metadata.
- This checkpoint is **independent** of the D4/D8/ADR-0072 technical reasons below: even if the architecture fit were perfect, the unresolved license would still bar copying source. Adopting *concepts* (the design lessons in §7) is unaffected — only verbatim/derived source is gated.

Lifting this gate requires upstream clarification (relicense, dual-license clarification, or an explicit BSD statement on `nostrdb-rs`).

## 6. RECOMMENDATION

**Reject `nostrdb-rs`. Keep the hand-rolled `LmdbEventStore` path targeting `nostr-lmdb` per ADR-0072.**

Decisive reasons (any one is sufficient; all four hold):

1. **D4:** nostrdb owns its own ingester+writer threads and fixed insert policy; `process_event` is fire-and-forget `Result<()>`. NMP cannot be the single writer per fact nor interpose its insert invariants (foreign pre-tombstones, provenance merge, claim-GC, NIP-40, kind:5 self-delete-only, `InsertOutcome`, ADR-0072 emit).
2. **ADR-0072:** `Ndb::new` creates the LMDB env internally with no injection; single-commit atomicity across NMP's secondaries is impossible without the already-rejected two-env fallback.
3. **D8:** NMP's composite reverse index — which on each insert names exactly the interested views to wake — is NMP-owned and inexpressible in nostrdb's flat filter-poll subscription model (`subscribe` + `poll_for_notes`). (Note: the visitor/allocation objection from the original draft is withdrawn — current `nostrdb-rs` exposes `fold` / `try_fold` early-stopping scans; it is no longer a rejection ground.)
4. **License:** `nostrdb-rs` publishes `GPL-3.0-or-later` while the C repo appears BSD; the ambiguity is unresolved (§5b). Code-level borrowing is blocked until cleared — independent of the technical reasons above.

Risk note: the "battle-tested code for free" upside is real but does not survive contact with D4/D8/ADR-0072 — the invariants we would have to rebuild *above* nostrdb are exactly the ones that carry the correctness load, and we would carry an FFI fork on top.

## 7. What to carry over conceptually (we keep the lessons, not the dep)

The hand-rolled path should adopt these *as Rust design*, not as a dependency:

- **Visitor query semantic** (lessons §2.3): the `EventStore` trait already returns lazy `Box<dyn EventIter>` (see `events.rs`); add a `query_visit(filter, FnMut(&StoredEvent) -> Continue|Stop)` variant as the default for view-internal scans so `recompute_full` stops at the view limit with zero buffer. This is the §2.3 win, kept on our terms.
- **Separable mutable metadata table** (lessons §2.2): persist the `Projections` cache as an NMP sub-db keyed `(namespace, key)` under the *single ADR-0072 env*, in-place updated on insert. Removes the restart aggregate-recompute cliff.
- **strfry-style packed note layout** (lessons §2.1): defer. A future optimization on the hand-rolled path *if* `bincode`-in-value read cost shows up in M3 benchmarks; not v1-blocking.
- **Single-writer discipline:** keep NMP's existing model — all mutation through `EventStore::insert` behind the actor (already D4-correct in `mem/insert.rs`). nostrdb validates the *shape* of this design; we already have it.
- (notedeck-side patterns — `SubKey`, `(owner,key,scope)`, compaction/leg split — are tracked separately in `nostrdb-notedeck-lessons.md` §4 and unaffected by this decision.)

## 8. Follow-ups

The conceptual techniques carried over in §7 are tracked as concrete implementation issues. The decision-refresh task itself is #1515.

| Issue | What it implements |
|---|---|
| #1515 | This decision refresh — corrects the stale visitor objection, states direct-adoption blockers concretely, adds the license checkpoint, cross-links follow-ups. |
| #1516 | True streaming LMDB `query_visit` (§7 visitor semantic; lessons §2.3) — `query_visit(filter, FnMut(&StoredEvent) -> Continue|Stop)` as the default for view-internal scans. |
| #1517 | Audit and tighten cache `StoreQuery` planning/index coverage — correctness for ADR-0070 offline/cache-serve coverage and intentional index paths. |
| #1518 | Queryable relay provenance indexes without splitting store ownership — provenance derived from insert-owned sidecars, single-writer (§7 no split-ownership). |
| #1519 | Persist insert-owned projection metadata sidecars (§7 separable metadata; lessons §2.2) — restart-safe cache projections without recomputing aggregates from scratch. |
| #1520 | Event-driven store notification/replay wakeups — correct missed wakeups, no polling-based reactivity (§7 / lessons §2.4 subscription wakeup model). |
| #1521 | Map LMDB cache health failures to diagnostics — typed diagnostics for map/reader/migration health. |
| #1522 | Capture cache adaptation baselines and fixtures before any performance-affecting PR (precondition for #1516 and others). |
| #1524 | Final cache adaptation acceptance gates and regression thresholds. |

M3 implementation proceeds against `nostr-lmdb` + ADR-0072 env-injection as already planned.
