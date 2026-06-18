# ADR-0058 — Cursor-based event-log consumption (the "pull" model)

- **Status:** Accepted pending implementation (design ratified; PR ladder queued — see §8).
  **Revised 2026-06-18 (Revision 2 — pre-implementation hardening): an adversarial
  codex review found six design flaws that would bake polling, unbounded retention,
  or an incomplete mutation log into step-1. See "Revision 2" below, which amends the
  primitive (§3), the store-infra seq-allocation rule (§4), the duplicate/provenance
  contract (§5), retention/GC (§6), the ADR-0039 reconciliation (§6.1), and the
  ladder (§8). The body sections below are amended inline where they were factually
  wrong; Revision 2 carries the authoritative contract.**
- **Date:** 2026-06-18
- **Doctrine:** `doctrine:d1` (reads-through-store), `doctrine:d4` (single writer),
  `doctrine:d5` (bounded), `doctrine:d8` (no polling)
- **Related:**
  - **ADR-0039** (push projection seam is canonical; reject generic pull accessors) —
    this ADR does **not** reverse it. ADR-0039 governs **host consumption of
    kernel-derived projections**; that stays PUSH. This ADR governs a **different
    layer** — consumption of the raw **event log** by an external mirror, and the
    store-cursor mechanism *underneath* UI "give me more". See §6.1.
  - **ADR-0045** (store→projection replay / cache-serve) — the cursor reuses the
    same durable store this seam already replays from.
  - **#1523** (adopt nostrdb cache lessons) — its subissues are the **store
    foundation** this builds on: #1516 streaming `query_visit` (landed), #1518
    relay×kind provenance index (landed), #1520 event-driven cache-serve wakeups
    (in flight, PR #1541), #1519 insert-owned sidecars (open), #1517 index
    coverage (open).
  - **#1552** (raw-event-tap elimination) — stripped the speculative native push
    sink (C-ABI register/ack + retain-until-ack cursor + `created_at` store-resync).
    This ADR is its replacement: external per-event consumption becomes a pull
    cursor, not a push callback. The forward-references in `docs/escape-hatches.md`,
    `docs/architecture/external-consumers.md`, and `docs/aim.md` §1 ("a bounded,
    backpressured pull cursor — forthcoming work") point here.

## Revision 2 (2026-06-18) — pre-implementation hardening

This revision is the authoritative contract for the work tracked in **#1566**. It
was written *before* commissioning step-1, in response to an adversarial pre-build
codex review (verdict: "not sound as-is"). The Rev 1 *intent* stands — a durable,
arrival-ordered log cursored at the consumer's pace, woken not polled — but six
load-bearing details were wrong or under-specified. Three of them rest on a false
premise about current code (verified against master):

- `GcBudget::production()` returns `Default`, and `Default` sets
  `max_total_events = usize::MAX` (`crates/nmp-store/src/types/gc.rs:34,65,76`).
  **Durable LRU eviction is OPT-IN and OFF by default in production**;
  `HOT_EVENT_CEILING` is merely an explicit-retention alias
  (`crates/nmp-store/src/types/gc.rs:8-18`). Rev 1 §6's "production uses
  `HOT_EVENT_CEILING = 10_000`" premise is **false** and is corrected here (R2.4).
- kind:5 deletion removes its targets as **store-internal side effects** and returns
  only `InsertOutcome::Inserted` (`crates/nmp-store/src/mem/insert_kind5.rs:34-38,88-90`;
  `crates/nmp-store/src/lmdb/insert_kind5.rs`). Log rows **cannot** be derived from
  the public `InsertOutcome` (R2.2).
- `EventStore::gc_step_with_pins` takes an event-id pin set plus `CoverageGuard`s
  (`crates/nmp-store/src/events.rs:383-427`) — it has **no** `protected_log_floor_seq`
  seam. Rev 1 §6 assumed one that does not exist (R2.3, R2.4).

The amendments below supersede the corresponding Rev 1 text. Where a body section
still reads the old way, it is corrected inline and points here.

### R2.1 Level-triggered cursor/wake contract — the ADR-0039 reconciliation made airtight

**The flaw.** A bare wake is *edge-triggered*: it fires once when new data crosses
the cursor. If `pull_page` returns a full page while `latest_ingest_seq` is still
ahead of the consumer, no further edge need fire, and a consumer that wants the rest
is tempted to `sleep`-loop and re-poll — the exact D8 / ADR-0039 anti-pattern this
ADR claims to avoid. Rev 1 §6.1's reconciliation was therefore not airtight.

**The contract (MUST).** Pull is event-driven only if the page result is
**level-triggered** — it tells the consumer how far it still is from caught-up, and
the registry re-wakes a lagging cursor instead of relying on a single edge:

1. **`PullPage` carries level state.** Every page returns
   `next_after_seq: u64` (the cursor to pass next), `latest_seq: u64` (the store's
   `latest_ingest_seq` at read time), and `has_more: bool`
   (`next_after_seq < latest_seq` after applying the page). A consumer learns it is
   behind **from the page itself**, never by re-asking on a timer.
2. **The consumer MUST drain.** On any wake (or on its own resume), a consumer calls
   `pull_page` repeatedly, advancing `after_seq := next_after_seq`, **until
   `has_more == false` (caught up) or its own per-tick budget is exhausted**. It does
   not sleep-and-recheck; it drains the level it was handed.
3. **Cursor registration emits an initial wake.** When a cursor is registered with
   `after_seq < latest_ingest_seq` (reconnect, cold start, a consumer that fell
   behind), the registry emits a wake **immediately**, so a behind-cursor never waits
   for the *next* ingest to discover it has backlog. This closes the "missed wake /
   reconnect" hole.
4. **A registered cursor is re-woken while lag remains.** If a consumer exhausts its
   budget with `has_more == true`, or a new ingest advances `latest_ingest_seq` past a
   registered cursor, the registry re-arms that cursor's wake on the next tick. Wake
   is thus *level-driven* (re-fires while `after_seq < latest_ingest_seq`), not a
   one-shot edge.

**Why this is NOT an ADR-0039 poll accessor.** ADR-0039 rejects a *projection* pull
accessor because "a pull accessor gives the host no signal for when the data changed,
so it forces a poll loop" (`docs/decisions/0039-...:32-40`). Here the consumer is
**never the source of the timing decision**: it acts only on a wake, and the wake is
guaranteed to (re-)fire exactly while the cursor is behind. The page's `has_more`
bounds a single drain burst; the registry's re-wake bounds when the next burst
starts. There is no `Task.sleep`, no timer, no "ask again in N ms." Host *projection*
consumption stays PUSH and is untouched (§6.1). This is the difference between
event-driven pull and the poll loop ADR-0039 killed, and it is now mechanical, not
aspirational.

### R2.2 Mutation-log taxonomy + hook points — the log is store-internal

**The flaw.** Rev 1 implied `StoreLogEntry` rows could be appended from the public
`InsertOutcome`. They cannot: kind:5 deletion removes targets as **internal side
effects** inside `insert_kind5` and the call still returns `InsertOutcome::Inserted`
(`crates/nmp-store/src/mem/insert_kind5.rs:34-38,88-90`; LMDB twin). A log built by
inspecting `InsertOutcome` would silently miss every NIP-09 target removal.

**The contract (MUST).** The ingest log is appended at **every primary mutation site
INSIDE the store**, under the same lock/txn as the mutation (R2.5), not derived from
any public return value. Two equivalent implementations are sanctioned; the
implementer picks one and uses it uniformly:

- **(a) Internal append helpers** — a private `append_log(&mut st, entry)` /
  `append_log_in_txn(txn, entry)` called at each mutation site (insert/replace, each
  kind:5 target removal, NIP-40 expiry purge, durable-LRU removal); or
- **(b) Mutation accumulator** — insert/delete/gc helpers return a
  `Vec<StoreLogMutation>` (or push into a passed-in sink) that the outer store method
  drains into the log within the same txn.

**`op` taxonomy (exact).** The log op enum is:

```
Inserted                         // a newly stored, accepted event
Replaced  { replaced_id }        // replaceable/parameterized-replaceable supersede
Deleted   { target_id, reason: DeleteReason }

DeleteReason = Nip09            // NIP-09 kind:5 semantic delete (self-delete of a target)
             | Nip40Expiry      // NIP-40 expiration purge
             | AdminPurge       // explicit DeleteFilter / admin/user purge
             | LruEviction      // durable-LRU removal (only when a finite ceiling is set)
```

**Which mutations produce rows.** `Inserted` and `Replaced` rows are produced on the
accepting insert/replace path (one `Replaced` carrying the superseded id; the kind:5
*event itself* is an `Inserted`). One `Deleted { target_id, Nip09 }` is emitted **per
removed target** inside `insert_kind5` (both the `e`-tag and `a`-tag arms). `Nip40Expiry`
and `LruEviction` rows are emitted by the GC pass when it purges/evicts a primary row.
**Duplicates produce no row** (see §5) and **address-only tombstones** (a kind:5 `a`-tag
that matched no stored event yet — `insert_kind5.rs:107-111`) produce no `Deleted` row,
because no stored event was removed; they remain pure tombstone state until a future
arrival is suppressed.

**Decision — `Nip40Expiry` and `LruEviction` DO get rows.** A mirror that holds only
distinct events still needs to learn an event left the authoritative set, regardless
of *why*, or it will diverge (keep a row the source dropped). `reason` lets a mirror
that only cares about NIP-09 filter the rest. This is also what makes R2.4's seq-keyed
log GC tractable: the log is the complete mutation history up to its own retention
floor, not a partial one.

### R2.3 Retention is its own seq-keyed log GC — not an event-id pin, not a K3 pin

**The flaw.** Rev 1 §6 threaded a `protected_log_floor_seq` "through the existing GC
pin input." That seam does not exist: `gc_step_with_pins` takes a `HashSet<EventId>`
plus `&[CoverageGuard]` (`crates/nmp-store/src/events.rs:383-427`). Both are
**event-id / (filter_hash, relay) keyed**; neither can carry a *seq* floor, and the
K3 coverage pins (ADR-0056) protect *primary event rows* for read-soundness — a
different concern from retaining *log rows* for a slow cursor.

**The contract.** Log retention is modelled as a **separate, seq-keyed GC over the
`nmp-ingest-log` sub-db**, independent of (and composed alongside, never merged into)
primary-event LRU and the K3 coverage-ledger pins:

- The log GC owns a single seq floor, `log_retention_floor_seq`. Rows with
  `seq <= log_retention_floor_seq` are eligible to be pruned from `nmp-ingest-log`
  (and `oldest_available_seq()` rises to match). It never touches primary event rows;
  primary LRU and K3 pins never touch the log.
- **`GapAllowed` cursor:** imposes no floor claim. If `after_seq` has fallen below
  `oldest_available_seq() - 1`, `scan_log_since_seq` returns
  `PullGap { requested_after_seq, first_available_seq }` — an explicit gap, **never a
  silent skip** (unchanged from Rev 1, but now defined against *this* log GC).
- **`Protected { max_lag_entries }` cursor:** an actor-owned cursor registration
  publishes a **retention claim** — the minimum `after_seq` across registered
  protected cursors becomes a *ceiling* on `log_retention_floor_seq` (the log GC may
  not prune past the slowest protected cursor). This is a property of the **log GC's
  own input**, a `min_protected_cursor_seq: Option<u64>`, NOT a value threaded into
  `gc_step_with_pins`. If a protected cursor's lag exceeds `max_lag_entries`, its claim
  is dropped and it degrades to the `GapAllowed` gap contract — so a stuck consumer
  cannot pin the log unbounded (D5).

This means R2.3 introduces a **new store seam** (the log GC + its
`min_protected_cursor_seq` input), specified as its own ladder step (§8 step-4), not a
reuse of the K3 pin path. It composes with K3: a `Protected` cursor whose raw bytes
let a mirror avoid pinning *primary* rows still needs its *log* rows retained, and the
log GC is the only thing that retains them.

### R2.4 Correct the false premise + bound the raw-event duplication (HIGH #5)

Rev 1 §6 opened with "Production runs LRU eviction (`GcBudget::production`,
`HOT_EVENT_CEILING = 10_000`)." **This is false.** `GcBudget::production()` is
`Default`, `max_total_events = usize::MAX` — durable LRU eviction is **off by default**
(`crates/nmp-store/src/types/gc.rs:34,55-78`); a finite ceiling is opt-in via
`with_durable_event_ceiling` and is explicit disk/user policy (ADR-0057 #1443/#1480).
The corrected §6 below no longer assumes eviction is on.

That correction *raises* a real concern (HIGH #5): because `Inserted`/`Replaced` rows
carry `raw_event` bytes, an always-on log with no GC and no eviction would **double the
durable event payload indefinitely** — every stored event also lives, verbatim, in the
log. So **step-1 MUST ship a bounded/prunable log policy** rather than defer it:

- The seq-keyed log GC (R2.3) is **part of step-1's contract**, with a default
  retention bound even when no cursor is registered (e.g. retain the log tail to a
  default `max_log_entries` / a size budget; prune below it via `log_retention_floor_seq`).
  An unregistered store keeps a bounded recent tail, not the whole history. **Or**
- raw-event writes into the log are **gated behind cursor registration / retention
  being wired** — the log carries ids only until a consumer that needs raw bytes
  registers, and the GC is in place before raw duplication is enabled.

The implementer picks one; either way **always-on unbounded raw duplication is
forbidden**. The mirror's "no need to pin primary rows, the log carries the bytes"
optimization (Rev 1 §6) is sound **only** under a bounded/GC'd log — the tradeoff is
"log rows retained to the slowest protected cursor," not "log grows forever."

### R2.5 DB-resident, txn-local seq allocation — not an `AtomicU64` (HIGH #4)

**The flaw.** "Allocate the seq in the event's write txn" is correct intent but the
proof in Rev 1 (§7 "actor single-writer") is **wrong**: the store is `Send + Sync`,
so the actor being single-threaded is not what serializes store writers. What
serializes them is **LMDB's single write transaction** and **Mem's mutex**.

**The contract (MUST).** `nmp-ingest-meta:last_seq` is **read and incremented INSIDE
the active event mutation transaction** — for LMDB the same `RwTxn` that writes the
event and NMP sub-dbs; for Mem under the same held mutex guard. **Never** via a
separate `AtomicU64`, and **never** by copying the existing LRU access-counter pattern
(that counter is bumped on *reads*, is not an append position, and lives outside the
write txn). Allocating outside the txn would let a crash commit an event without its
seq (or a seq without its event), breaking the total order / crash-recovery guarantee
the whole design rests on.

Two facts make this safe in practice: **cache-serve does not call `store.insert`** (it
feeds the post-store projection seam directly — ADR-0045 §1.2, R2.4(a); PR #1541
`cache_serve/continuation.rs`), so the replay half allocates **no** seq and cannot
perturb arrival order. **Local publish does** go through the same insert path, so its
events get seqs in arrival order like any other — correct and intended.

### R2.6 Backfill / migration policy for existing stores (Required Amendment #5)

**The flaw.** Rev 1 said nothing about an **existing** store that already holds events
but has no `nmp-ingest-log` (every device that upgrades into the log version).

**Decision — no backfill; the cursor starts at the current head.** On first open of a
store version that introduces the log, `last_seq` is initialized to `0` and the log is
empty; `latest_ingest_seq()` reflects only events ingested *after* the upgrade. A
freshly-registered cursor therefore starts at `oldest_available_seq()` (which equals
the post-upgrade head until new events arrive) and a `GlobalLog` mirror sees only
*new* arrivals through the cursor.

Rejected alternatives, with reasons:

- **Synthetic `Backfilled` rows for the whole existing store.** Rejected: it would
  fabricate an *arrival* order the store never recorded (existing rows have only
  `created_at`, which the ADR's whole §4 premise says is **not** arrival-monotonic),
  manufacturing exactly the false ordering the log exists to avoid. It also doubles
  every existing event's bytes into the log on upgrade (the HIGH #5 concern, at its
  worst).
- **A separate initial mirror scan.** Rejected for step-1 as the *log's* concern: a
  mirror that wants the pre-existing corpus already has the store's own export/query
  surface (the `hl` mirror does an initial nostrdb sync by other means). Coupling a
  one-time historical scan into the cursor primitive would burden it speculatively
  (D5). If a mirror genuinely needs "everything, then tail," that is a mirror-side
  concatenation of (export snapshot) + (cursor from head), specified in the step-5
  `hl` migration, **not** the step-1 log.

The log is thus an **arrival journal from the upgrade point forward**, which is exactly
what an arrival-ordered cursor can honestly provide.

### R2.7 The `load_older` claim is split out — step-1 is `GlobalLog`-only (HIGH #6)

**The flaw.** Rev 1's §8 step-6 treated `load_older → PullCursor` as a near-free reuse
of the global seq log. It is not: feed paging is **`created_at` / id ordered**
(`crates/nmp-feed/src/types.rs`, `.../root_indexed/engine/mod.rs`), `shape_to_store_queries`
maps interests to `created_at` indexes, and some shapes have **no time cursor at all**
(`crates/nmp-core/src/kernel/cache_serve/queries.rs:49-52`). A global `seq_be` scan
filtered by shape is **not** acceptable feed pagination (it would scan the entire log
to fill one interest's page, and seq order ≠ the display order the feed presents).

**The contract.**

- **`scan_log_since_seq` (step-1) is scoped to `GlobalLog` only.** The step-1 primitive
  serves the mirror. It does **not** claim to paginate an interest-scoped feed.
- **Interest-scoped feed pagination is a SEPARATE design**, deferred to its own step
  (§8 step-6) and **not solved by step-1**. It requires either an interest-scoped seq
  index (a per-shape arrival index, materialized lazily) or a **two-cursor model** (a
  `created_at`/id display cursor for ordering + a seq cursor only for the
  "did a late old event land behind me?" completeness check). Picking between those is
  step-6 design work, explicitly out of scope for the step-1 ADR contract.

The ADR no longer implies the global log "solves" `load_older`. The §8 ladder is
re-scoped accordingly.

### R2.8 Nice-to-haves promoted to MUST in the FFI contract

- **Hard FFI page-size caps.** `pull_page`'s `page_limit` is clamped to a hard
  per-call maximum at the FFI boundary (a bounded `PullPage`), so no host call can
  request an unbounded page and blow the D5 cross-FFI bound. The cap is a constant in
  the FFI layer, not a host-supplied value.
- **`pull_page` MUST NOT be called on the UI thread from a projection `apply()`
  callback.** It is a synchronous store read; calling it from inside the pushed-frame
  `apply()` would block the UI thread on a store scan. Documented as a hard
  consumer-side rule (and surfaced in the builder guide / the `hl` migration contract).

### R2.9 Sequencing (codex "Sequencing")

#1548 (insert sidecar) touches the same store insert/delete/GC paths and bumps the
LMDB sub-db count; land it before step-1 **or** develop step-1 directly on top of it,
to avoid two simultaneous sub-db-count migrations. #1541 (event-driven wakeups) does
not block step-1 but **should land before step-3 (FFI wake)**, which generalizes its
wake model. This is tactical ordering for #1566, not an ADR decision.

---

## 1. Problem

Two consumers want events at their own pace, and neither is served well by a push
callback:

1. **External event mirror** (the out-of-tree `hl` app's nostrdb mirror). It needs
   the distinct stored events, durably and in order, to mirror them into its own
   store. The retired raw event tap (#1552) and the briefly-built native push sink
   forced this through a below-seam callback with hand-rolled backpressure
   (retain-until-ack, `created_at` resync watermark, batch coalescing) that proved
   bug-prone (silent drop, hole-punching watermark, callback-under-lock deadlock).
   A slow consumer behind a push sink is a liability; a slow consumer pulling from
   the durable store is free.
2. **UI "give me more"** pagination (`load_older`). The feed already grows its
   window on demand (`crates/nmp-feed/.../engine`, `nmp_app_load_older_feed`,
   `crates/nmp-ffi/src/feed.rs`), but each feed re-implements windowing over
   `created_at`-ordered scans. `created_at` is **not arrival-monotonic** — a relay
   can deliver an old event late, landing it *behind* a `created_at` cursor, so a
   cursor over `created_at` silently skips it.

The shared need is a **durable, arrival-ordered log of accepted events** that a
consumer cursors over at its own pace, with a lightweight **wake** when new data
is available (so it never polls).

## 2. Decision

Introduce a **`PullCursor`** over a store-owned **ingest log**. A consumer holds a
cursor (its last consumed log position), calls `pull_page(cursor, scope, max)`, and
advances. An optional **wake signal** (the #1520 event-driven cache-serve wakeup
mechanism, generalized) tells the consumer "new data ≥ your cursor" without carrying
event data — so the cursor is **wake-driven, not polled** (D8). The wake is
**level-triggered** (Rev 2 R2.1): it re-fires while the cursor is behind and the page
itself reports `has_more`, so the consumer drains rather than sleep-rechecks. The
durable store
is the retention buffer; backpressure is intrinsic (a slow consumer pulls less; the
events wait where they already are).

This replaces the deleted native push sink (#1552) for external mirrors and becomes
the single mechanism underneath UI "give me more".

## 3. The primitive

```
PullCursor { consumer_id, scope: PullScope, after_seq: IngestSeq,
             mode: GapAllowed | Protected { max_lag_entries }, page_limit }

StoreLogEntry {
  seq: u64,                         // monotonic INGEST order (not created_at)
  op:  Inserted | Replaced { replaced_id }
     | Deleted { target_id, reason: DeleteReason },   // taxonomy — Rev 2 R2.2
  event_id,
  raw_event: Option<RawEvent>,      // present for Inserted / Replaced (bounded by R2.4)
  source_relay: Option<RelayUrl>,
  received_at_ms,
}

// Rev 2 R2.2 — every reason an event left the authoritative set; a mirror filters
// on `reason` (e.g. NIP-09-only) but is told regardless so it cannot diverge.
DeleteReason = Nip09 | Nip40Expiry | AdminPurge | LruEviction
```

- `Inserted` for accepted stored events; `Replaced` carries the new event + the
  superseded id (matches existing replaceable handling); NIP-09 emits the kind:5
  `Inserted` plus one `Deleted { _, Nip09 }` per removed target. **These rows are
  appended at the store-internal mutation site, NOT derived from the public
  `InsertOutcome`** — kind:5 removes targets as internal side effects and still
  returns `InsertOutcome::Inserted` (Rev 2 R2.2; `mem/insert_kind5.rs:34-38,88-90`).
- **Duplicates do not create log entries** — see §5.

`scan_log_since_seq` returns a **level-triggered** page (Rev 2 R2.1):

```
PullPage {
  entries: Vec<StoreLogEntry>,      // hard-capped at the FFI boundary (R2.8)
  next_after_seq: u64,              // pass as `after_seq` on the next call
  latest_seq: u64,                  // store's latest_ingest_seq at read time
  has_more: bool,                   // next_after_seq < latest_seq — consumer MUST drain
}
| PullGap { requested_after_seq, first_available_seq }   // explicit, never a silent skip
```
- `PullScope` is `GlobalLog` (the mirror) or an `InterestShape` (a feed window),
  composed with the existing `shape_to_store_queries` mapping
  (`crates/nmp-core/src/kernel/cache_serve/queries.rs`).

Store API additions (ascending `seq > after_seq`, the order mirrors and crash
recovery need):

```
latest_ingest_seq() -> u64
oldest_available_seq() -> Option<u64>
scan_log_since_seq(after_seq, limit) -> PullPage
```

FFI: a **synchronous read-only** `pull_page` call (fits the fire-and-forget actor
model — dispatch stays one-way; state still crosses through frames/projections) and
a typed `nmp.pull.wake { cursor_id, latest_seq }` projection — wake is a signal, not
event data, consistent with ADR-0037 typed sidecars.

## 4. Store infrastructure

`created_at` is not a cursor. A durable consumer cursor needs a **total order over
arrival**, which the store does not have today: every scan is ordered by
`created_at` and the only `seq u64` is the LRU access counter (bumped on reads, not
an append position).

- **`MemEventStore`**: add `ingest_seq: u64` + `ingest_log: BTreeMap<u64,
  StoreLogEntry>` inside the existing locked state.
- **`LmdbEventStore`**: add sub-dbs `nmp-ingest-log` (`seq_be -> StoreLogEntry`) and
  `nmp-ingest-meta` (`last_seq`). **`nmp-ingest-meta:last_seq` is read and incremented
  INSIDE the active event mutation txn** — for LMDB the same `RwTxn` that writes the
  event and NMP sub-dbs; for Mem under the same held mutex guard. **Never** via a
  separate `AtomicU64`, and never by copying the LRU access-counter (which is bumped
  on reads, is not an append position, and lives outside the write txn). The
  serializing invariant is LMDB's single write transaction / Mem's mutex — **not** the
  actor being single-threaded (the store is `Send + Sync`). Cache-serve replay does
  **not** call `store.insert` so it allocates no seq; local publish does, and gets a
  seq in arrival order like any other insert. See Rev 2 R2.5.

## 5. Duplicates / provenance

The pull primitive delivers **distinct stored events, once** — not per-arrival
duplicates. The mirror contract is therefore **"distinct events plus an optional
merged-provenance read, not traffic replay"** (Rev 2): a mirror reads each distinct
event once and, if it wants the relay set that delivered it, reads the merged
provenance index (#1518/#1535) — it does not receive one log row per relay arrival.
Codex confirmed this is safe for `hl`, whose nostrdb mirror needs distinct verbatim
signed frames, not relay-arrival telemetry. Rationale:

- The store doctrine is distinct-event storage with provenance merging (aim §4.1);
  the relay×kind provenance index (#1518/#1535) already records all non-private
  relay sources for an event.
- UI does not need duplicates. A nostrdb **mirror** needs idempotent event state,
  not relay-traffic telemetry.

The one capability the raw tap had that this drops is *per-arrival* delivery
(duplicate fan-out with source-relay provenance). No real consumer needs it: the
mirror mirrors distinct events; the in-process relay-forwarding policy (the
`ExternalEventSinkPolicy` dispatcher kept by #1552) still handles republish. If a
future consumer genuinely needs per-arrival data, add a separate `arrival_log
{ arrival_seq, event_id, source_relay, outcome }` under its own issue/ADR — **do not
burden this primitive speculatively** (D5: no invented mechanism).

## 6. Retention / GC

> **Rev 2 correction.** The Rev 1 text here assumed production runs durable LRU
> eviction at `HOT_EVENT_CEILING = 10_000` and threaded a `protected_log_floor_seq`
> "through the existing GC pin input." **Both are wrong.** `GcBudget::production()`
> is `Default` with `max_total_events = usize::MAX` — durable LRU eviction is **OFF
> by default** (`crates/nmp-store/src/types/gc.rs:34,65,76`); `HOT_EVENT_CEILING` is
> only an explicit-retention alias. And `gc_step_with_pins` takes event-id pins +
> `CoverageGuard`s (`events.rs:383-427`) — it has **no** seq-floor seam. The corrected
> model below (and Rev 2 R2.3 / R2.4) replaces this section.

Log retention is a **separate, seq-keyed GC over `nmp-ingest-log`**, independent of
primary-event LRU and the K3 coverage-ledger pins (both of which are event-id /
`(filter_hash, relay)` keyed and cannot carry a seq floor). It owns one floor,
`log_retention_floor_seq`; rows with `seq <= log_retention_floor_seq` are prunable and
`oldest_available_seq()` rises to match. It never touches primary event rows; primary
LRU and K3 pins never touch the log. Two cursor modes:

- **`GapAllowed`**: if `after_seq < oldest_available_seq() - 1`, the page returns
  `PullGap { requested_after_seq, first_available_seq }` — an explicit gap, **never a
  silent skip**.
- **`Protected { max_lag_entries }`**: an actor-owned cursor registration publishes a
  **retention claim** — the minimum `after_seq` across registered protected cursors is
  a *ceiling* on `log_retention_floor_seq` (the log GC may not prune past the slowest
  protected cursor). This is an input to the **log GC** (`min_protected_cursor_seq`),
  **not** a value passed to `gc_step_with_pins`. If lag exceeds `max_lag_entries`, the
  claim is dropped and the cursor degrades to the explicit gap contract (D5: a stuck
  consumer cannot pin the log unbounded).

**Bounded raw-event duplication (Rev 2 R2.4, HIGH #5).** Because `Inserted`/`Replaced`
rows carry the raw event bytes, an always-on log would *double* the durable event
payload. Step-1 therefore MUST ship the bounded log policy above — a default retention
bound even with no cursor registered (retain a bounded recent tail), **or** gate
always-on raw-log writes behind retention being wired. Always-on unbounded raw
duplication is forbidden. The "mirror need not pin primary rows, the log carries the
bytes" optimization is sound **only** under this bounded/GC'd log: log rows are
retained to the slowest protected cursor, not forever. UI rows stay protected by
existing interest/feed claims. The log GC composes *alongside* the K3 coverage-ledger
pin machinery (ADR-0056); it does not reuse or duplicate it.

### 6.1 Reconciliation with ADR-0039 (the load-bearing point)

ADR-0039 rejects a **generic pull accessor for kernel-derived projections** because
"a pull accessor gives the host no signal for when the data changed, so it forces a
poll loop." That reasoning is fully preserved here:

- **Host projection consumption stays PUSH.** This ADR adds **no** projection pull
  accessor, no `nmp_app_get_snapshot`. The snapshot/projection frame remains the one
  canonical way a host reads derived view state.
- **This ADR operates a different layer**: the raw **event log** (for an external
  mirror) and the store-cursor *underneath* a feed's "give me more". The feed still
  surfaces to the host via its existing **push** projection — `load_older` becoming a
  `PullCursor` wrapper is an internal mechanism swap, invisible to the host seam.
- **The exact anti-pattern ADR-0039 named is avoided**, and Rev 2 makes this
  mechanical rather than aspirational. A *bare* wake is edge-triggered, and an
  edge-triggered cursor can still degenerate into a poll loop after a reconnect, a
  missed wake, or a partial drain (if `pull_page` returns a full page while
  `latest_seq` is still ahead, no new edge need fire). The **level-triggered contract
  (Rev 2 R2.1)** closes that hole: `PullPage` carries `next_after_seq` / `latest_seq` /
  `has_more`; the consumer MUST **drain** until `has_more == false` or its budget is
  spent; cursor registration emits an **initial wake** whenever
  `after_seq < latest_ingest_seq`; and a registered cursor is **re-woken while lag
  remains**. The consumer is therefore never the source of the timing decision — it
  acts only on a (re-)firing wake and drains the level it was handed. No `Task.sleep`,
  no timer, no "ask again in N ms." This supplies precisely the change-signal ADR-0039
  said pull lacked, and is what keeps pull event-driven rather than a poll accessor.

"Pull" here therefore names a distinct concept from ADR-0039's rejected
"projection pull accessor"; the two ADRs are complementary, not in tension.

## 7. Doctrine alignment

- **D1** reads-through-store — the cursor reads the durable store, no side fetch.
- **D4** single writer — the ingest-seq is allocated **inside the event's own write
  txn** (LMDB `RwTxn` / Mem mutex, not an `AtomicU64` — Rev 2 R2.5); the log has one
  writer (the ingest chokepoint, ADR-0057). The serializing invariant is the write
  txn / mutex, not actor single-threading (the store is `Send + Sync`).
- **D5** bounded — pull is intrinsically backpressured; `max_pending`-style push
  state machines are deleted. No speculative per-arrival log. The log itself is
  bounded by its own seq-keyed GC (Rev 2 R2.3/R2.4) so raw-event duplication is never
  unbounded.
- **D8** no polling — the **level-triggered** wake + drain contract (Rev 2 R2.1) makes
  consumption event-driven: the consumer acts only on a (re-)firing wake and drains
  the level it is handed, never on a timer.

## 8. Implementation ladder (the remaining work)

The store **foundation** overlaps #1523 and is partly landed. The remaining,
pull-specific work is tracked in **#1566** (a #1523 subissue):

> **Rev 2 re-scope.** Step-1 is `GlobalLog`-only, ships its bounded log GC inside the
> same PR (so raw-event duplication is never unbounded), appends log rows at the
> store-internal mutation sites (not from `InsertOutcome`), and allocates seq inside
> the event txn. The log GC / retention is its **own** step (now step-5), not a reuse
> of the K3 pin path. `load_older` is **not** solved by step-1 and gets its own
> separate-design step (now step-7).

1. **Store ingest-seq index + log (`GlobalLog`) + `scan_log_since_seq`** on both
   backends (Mem + LMDB). Seq allocated **inside the event mutation txn** (R2.5); log
   rows appended at **store-internal mutation sites** including each kind:5 target
   removal (R2.2); `op` taxonomy with `DeleteReason` (R2.2); `PullPage` carries
   `next_after_seq` / `latest_seq` / `has_more` (R2.1); a **default bounded log
   policy** so raw duplication is bounded even with no cursor (R2.4). Parity tests:
   late old event after cursor; duplicate → no seq; `Replaced` op; NIP-09 `Deleted`
   ops (per target); `Nip40Expiry` / `LruEviction` ops; LMDB reopen seq continuity;
   no-backfill on upgrade (R2.6). *Smallest independently-valuable PR; the keystone.*
2. **Kernel pull service** over the `GlobalLog` scope (no FFI yet). The
   level-triggered drain + initial-wake / re-wake registry semantics (R2.1) live here.
3. **FFI `PullPage` + typed `nmp.pull.wake`** (generalize the #1520 wake). Hard FFI
   page-size cap; document `pull_page` must not run on the UI thread from `apply()`
   (R2.8).
4. **Cursor registration + retention modes** — `GapAllowed` explicit-gap contract +
   `Protected { max_lag_entries }` publishing `min_protected_cursor_seq` to the log GC,
   with explicit gap / lag-degrade tests.
5. **Seq-keyed log GC** (`log_retention_floor_seq`, the separate store seam — R2.3),
   composed alongside (never merged into) primary LRU and the K3 coverage pins.
6. **`hl` mirror migration** onto the `GlobalLog` cursor (mirror contract = distinct
   events + optional merged-provenance read, not traffic replay); then confirm the
   #1552 native sink stays deleted.
7. **`load_older` → interest-scoped pagination — SEPARATE DESIGN (R2.7).** *Not* a
   thin wrapper over the global seq log. Requires an interest-scoped seq index or a
   two-cursor model (a `created_at`/id display cursor + a seq cursor for the
   late-old-event completeness check). Design step in its own right; feed display
   order unchanged.

## 9. Risk

Bigger in **store** work (seq allocation, multi-op logging, LMDB schema migration,
GC log-floor) — but **simpler and safer at runtime**: no retain-until-ack
dispatcher, no hollow batches, no `created_at` watermark holes, no push-side
backpressure state machine. Crash recovery is "resume from seq." The hard part moves
to the correct layer — the durable store.
