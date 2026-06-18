# ADR-0058 — Cursor-based event-log consumption (the "pull" model)

- **Status:** Accepted pending implementation (design ratified; PR ladder queued — see §8).
  **Revised 2026-06-18 (Revision 2 — pre-implementation hardening): an adversarial
  codex review found six design flaws that would bake polling, unbounded retention,
  or an incomplete mutation log into step-1. See "Revision 2" below, which amends the
  primitive (§3), the store-infra seq-allocation rule (§4), the duplicate/provenance
  contract (§5), retention/GC (§6), the ADR-0039 reconciliation (§6.1), and the
  ladder (§8). The body sections below are amended inline where they were factually
  wrong; Revision 2 carries the authoritative contract.**
  **Revised again 2026-06-18 (Revision 3 — residual hardening): a second adversarial codex
  review raised three blocking residuals — `LruEviction`-as-`Deleted` is a mirror
  correctness trap, `delete_by_filter` / `AdminPurge` hook coverage was missing, and the
  ladder contradicted itself on whether the bounded log GC ships in step-1. Rev 3 fixes all
  three in place (taxonomy §3 / R2.2, hook list R2.2, ladder §8) and adds the "Revision 3"
  section below, authoritative where it supersedes Rev 2.**
  **Revised again 2026-06-18 (Revision 4 — mirror semantic-delete contract): a fourth
  adversarial codex review found one residual deep hole. Because retention removals (LRU
  eviction, `delete_by_filter(ByRelayOnly)`) destroy the primary event row yet emit no
  `Deleted` row, a durable mirror is necessarily a SEMANTIC SUPERSET of the store, not an exact
  current-store replica — and a semantic delete (NIP-09 / NIP-40) that arrives AFTER the target
  was already retention-evicted removes nothing in the store, so it produces no `Deleted` row,
  and a mirror relying on `Deleted` rows would keep the dead event forever. Rev 4 closes this by
  making the mirror apply NIP-09 / NIP-40 ITSELF from the log stream / its own copy (§5,
  step-5) and reframing `Deleted` rows as a best-effort optimization for events the store still
  holds, not the mirror's source of truth for deletion. See the "Revision 4" section below,
  authoritative where it supersedes Rev 3.**
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
Deleted   { target_id, reason: DeleteReason }   // SEMANTIC removal only — Rev 3

DeleteReason = Nip09            // NIP-09 kind:5 semantic delete (self-delete of a target)
             | Nip40Expiry      // NIP-40 expiration purge (event meant to be gone)
             | AdminPurge       // destructive delete_by_filter (ByAuthor/ByIds/ByKindRange)
// Rev 3: LRU eviction and delete_by_filter(ByRelayOnly) are RETENTION-class — the event
// still validly exists, so they emit NO Deleted row (see the Decision below). A mirror
// acts on a Deleted row by deleting its copy; it must NEVER do so on retention eviction.
```

**Which mutations produce rows.** `Inserted` and `Replaced` rows are produced on the
accepting insert/replace path (one `Replaced` carrying the superseded id; the kind:5
*event itself* is an `Inserted`). One `Deleted { target_id, Nip09 }` is emitted **per
removed target** inside `insert_kind5` (both the `e`-tag and `a`-tag arms). `Nip40Expiry`
rows are emitted by the GC pass when it purges an expired primary row. **`AdminPurge`
rows are emitted by `delete_by_filter`** (`crates/nmp-store/src/events.rs:363`), one per
removed primary row, under the same lock/txn as the removal — for the **destructive**
variants `ByAuthor` / `ByIds` / `ByKindRange` (operator intent to remove). The
`ByRelayOnly` variant is relay-source bookkeeping, not a destroy (the event still validly
exists elsewhere), so it is retention-class and emits **no** consumer-visible `Deleted`
row — like LRU eviction, which **emits no row at all** (Rev 3; see the Decision below).
**Duplicates produce no row** (see §5) and **address-only tombstones** (a kind:5 `a`-tag
that matched no stored event yet — `insert_kind5.rs:107-111`) produce no `Deleted` row,
because no stored event was removed; they remain pure tombstone state until a future
arrival is suppressed.

**Decision (Rev 3) — `Deleted` rows are SEMANTIC removals only; retention eviction emits
no row.** A `Deleted` row is a signal a mirror **acts on** by deleting its own copy, so
it must mean "this event is genuinely gone from the authoritative set," never "this store
dropped it for capacity or relay bookkeeping." Therefore:

- **`Nip09`, `Nip40Expiry`, and `AdminPurge` DO get rows** — all three are semantic
  removals: the author deleted it (kind:5), the event's own signed `expiration` tag fired
  (NIP-40 — meant to be gone), or an operator purged it on purpose (`delete_by_filter`
  `ByAuthor`/`ByIds`/`ByKindRange`). A mirror deletes its copy on these; `reason` lets a
  mirror that only honors NIP-09 filter the other two.
- **LRU eviction does NOT get a `Deleted` row at all** — an evicted event still validly
  exists, so a `Deleted { LruEviction }` would steer a mirror to **destroy valid data**
  (the trap Rev 2 fell into). `LruEviction` is **removed from `DeleteReason`**.
  Retention/eviction is invisible to the consumer by construction: a `Protected` cursor's
  log-floor pin (R2.3) prevents eviction of **unconsumed** log rows, and a consumer that
  falls past `max_lag_entries` receives an explicit `PullGap` (re-sync from
  `first_available_seq`), never a false delete. `delete_by_filter(ByRelayOnly)` — the
  event still exists, only its sole relay source was dropped — is in this same retention
  class and likewise emits **no** consumer-visible `Deleted` row.

The earlier "the log must record every removal regardless of *why* to stay a complete
mutation history" reasoning is **withdrawn**: log-GC tractability comes from seq-keying
(R2.3), not from logging retention evictions. The mirror contract is therefore
unambiguous — **a mirror NEVER deletes its copy on a retention eviction.** **Rev 4 sharpens
the positive half:** a `Deleted` row is only ever emitted for an event the producing store
**still held** at delete time, so it is a best-effort optimization, **not** a durable mirror's
source of truth for deletion. A durable mirror is a semantic superset of the store and **MUST
apply NIP-09 / NIP-40 itself** (from the kind:5 `Inserted` row and the held event's
`expiration` tag), so that a delete racing retention eviction — which removes nothing in the
store and emits no `Deleted` row — is still applied by the mirror (see §5 and the "Revision 4"
section).

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
`min_protected_cursor_seq` input), not a reuse of the K3 pin path. Its **bounded floor
ships in §8 step-1**; the `Protected`-cursor floor-pin that raises that floor to the
slowest protected cursor composes on top in **§8 step-4**. It composes with K3: a
`Protected` cursor whose raw bytes
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
  (D5). If a mirror genuinely needs "everything, then tail," the sound procedure
  (specified in the §8 step-5 `hl` migration) is: **register the cursor at the current
  head FIRST, then export the current store, then pull from that cursor** — tolerating
  duplicates but **never** gaps (cursor-first guarantees no arrival lands in the window
  between export and registration). That is mirror-side composition of (export snapshot)
  + (cursor from head), **not** a step-1 log concern.

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

## Revision 3 (2026-06-18) — residual hardening

A second adversarial codex review (verdict: "not yet sound to execute") raised three
BLOCKING residuals against Revision 2, all verified against code. Rev 3 closes them by
correcting the taxonomy (§3 / R2.2), the hook list (R2.2), and the ladder (§8) **in
place** — the sections above now read correctly. This section records the decisions.

### R3.1 `Deleted` rows are SEMANTIC removals only — LRU eviction emits no row

Rev 2 made `LruEviction` a `Deleted` reason and argued a mirror "needs to learn an event
left the authoritative set, regardless of *why*." That is a **correctness trap**: LRU
eviction is not a Nostr semantic delete — the evicted event still validly exists and the
store writes **no** tombstone for it (durable LRU is off by default,
`crates/nmp-store/src/types/gc.rs:34,65,76`; eviction creates no tombstone) — so a mirror
that acted on the row would **destroy valid data**. **Decision: `LruEviction` is removed
from `DeleteReason` entirely** — not kept as an advisory marker (an advisory row invites
exactly the confusion; dropping it is the cleaner, footgun-free choice). Retention is made
invisible to the consumer by construction: a `Protected` cursor's log-floor pin (R2.3)
keeps **unconsumed** log rows from being pruned, and a consumer that falls past
`max_lag_entries` gets an explicit `PullGap` (re-sync from `first_available_seq`), never a
false delete. `DeleteReason` is now `Nip09 | Nip40Expiry | AdminPurge` — all three are
**semantic** ("this event is genuinely gone from the authoritative set"): author delete,
NIP-40 expiry (the event's own signed `expiration` fired), operator purge. **Mirror
contract: a mirror deletes its copy ONLY on `Nip09` / `Nip40Expiry` / `AdminPurge`, NEVER
on retention eviction.** Corrected inline in §3, R2.2, and §6.

### R3.2 `delete_by_filter` / `AdminPurge` hook coverage

`EventStore::delete_by_filter(DeleteFilter)` (`crates/nmp-store/src/events.rs:363`; e.g.
`DeleteFilter::ByRelayOnly`, exercised at `lmdb/tests_kind5.rs:191`) is a primary mutation
site the Rev-2 hook list and parity tests omitted. **It is now in the hook list (R2.2) and
the step-1 parity tests (§8):** it emits one log row per removed primary row, under the
same lock/txn as the removal. Its `DeleteReason` follows R3.1 — the **destructive**
variants `ByAuthor` / `ByIds` / `ByKindRange` (operator intent) emit `Deleted { AdminPurge }`;
**`ByRelayOnly`** is relay-source bookkeeping (the event still validly exists elsewhere)
and is **retention-class — it emits NO consumer-visible `Deleted` row**, exactly like LRU
eviction. This keeps the mirror contract sound: no relay-cleanup or capacity operation can
make a mirror destroy a valid event.

### R3.3 Ladder consistency — the bounded log GC ships in step-1

Rev 2 contradicted itself: R2.4 put the bounded seq-keyed log GC **in** step-1, while §8
listed "log GC / retention" as a separate later step — reopening the unbounded
raw-event-duplication risk. **Resolved: the bounded log GC — the `log_retention_floor_seq`
floor mechanism plus a default retention bound — ships inside step-1.** There is no
standalone "log GC" step. The only retention work deferred is the **advanced
`Protected`-cursor floor-pin** (`min_protected_cursor_seq`, a ceiling on the step-1 floor),
which composes in step-4. **Step-1 never ships always-on unbounded raw-event log writes.**
The §8 ladder is renumbered accordingly: the standalone GC step is gone, so `hl` migration
is now step-5 and `load_older` is step-6.

### R3.4 `hl` initial full-sync procedure (R2.6 refinement)

The §8 step-5 `hl` migration specifies initial full sync as: **register the cursor at the
current head, then export the current store, then pull from that cursor** — tolerating
duplicates but **never** gaps. Cursor-first guarantees no arrival lands in the window
between export and registration.

### Untouched (closed in Rev 2, confirmed still closed)

R2.1 (level-triggered wake + drain + initial-wake + re-wake while lag remains), R2.5
(txn-local DB-resident seq allocation, not an `AtomicU64`), R2.7 (`load_older` split out —
step-1 is `GlobalLog`-only), and R2.8 (hard FFI page caps + no-UI-thread-pull) are
**unchanged** by Rev 3.

---

## Revision 4 (2026-06-18) — mirror semantic-delete contract

A fourth adversarial codex review confirmed Rev 3 closed its three blockers (the
`LruEviction` false-delete trap, `delete_by_filter` destructive-hook coverage, the ladder
step-1 bounded log GC) with **no** regression on R2.1 / R2.5 / R2.7 / R2.8 — and found **one**
residual deep hole, fixed here.

### R4.1 The hole: a semantic delete that races retention eviction emits no `Deleted` row

Rev 3 correctly made retention removals (LRU eviction, `delete_by_filter(ByRelayOnly)`) emit
**no** `Deleted` row, because the event still validly exists elsewhere. But `ByRelayOnly` is
**destructive of the primary event row**, not provenance-only: `crates/nmp-store/src/mem/insert.rs:147`
builds `ids_to_remove`, `crates/nmp-store/src/lmdb/delete.rs` `by_relay_only` removes it, and
`crates/nmp-store/src/lmdb/tests_kind5.rs:186-218` asserts the row is gone. So a durable mirror
that **keeps** retention-evicted events is necessarily a **SEMANTIC SUPERSET** of the store, not
an exact replica of its current authoritative set.

That superset opens a hole Rev 3 does not close:

1. Event `E` is logged (`Inserted`) and mirrored.
2. `E` is removed locally by LRU eviction or `delete_by_filter(ByRelayOnly)` — correctly **no**
   `Deleted` row (`E` still validly exists).
3. Later a NIP-09 kind:5 (or NIP-40 expiry) for `E` is observed. The store's kind:5 handler
   removes **per removed target** — but `E`'s primary row is **already absent locally**, so it
   removes nothing → emits **no** `Deleted { Nip09 }` row (`crates/nmp-store/src/mem/insert_kind5.rs`
   only removes/tombstones-with-removal inside the `target present` branch; the LMDB twin only
   emits a `Deleted` row when `target_stored`).
4. A mirror that relied on `Deleted` rows never learns `E` is semantically dead → it **keeps a
   deleted event forever** (divergence; privacy-relevant if `E` was a since-deleted note).

### R4.2 The fix: the mirror is a SEMANTIC SUPERSET and applies NIP-09 / NIP-40 itself

Adopted (codex's sound first option), specified precisely in §5 and step-5:

- **The mirror is a SEMANTIC SUPERSET, not an exact current-store replica.** It keeps events
  the store retention-evicts. Stated plainly in §5, step-5, and §3.
- **Because it is a superset, the mirror MUST apply Nostr deletion semantics ITSELF** and MUST
  NOT rely on the store's `Deleted` rows for correctness:
  - **NIP-09:** the mirror receives every kind:5 as a normal `Inserted` log row (kind:5 is an
    accepted event), and applies it to **its own** copy — deleting the target ids from its own
    store — exactly as any Nostr client does, regardless of whether the producing store still
    held the target. This is what makes the step-3 race resolve correctly.
  - **NIP-40:** the mirror applies expiry locally from the `expiration` tag on the events it
    holds, regardless of whether the producing store already expired them.
- **`Deleted` rows are reframed as a best-effort signal/optimization for events the store still
  held** — not the mirror's source of truth for deletion. A consumer that only tracks the
  store's current authoritative set MAY use them; a durable superset mirror MUST additionally
  apply NIP-09 / NIP-40 itself.

### R4.3 What the store emits on a kind:5 whose target row is already absent

**Decision (stated explicitly):** the store emits the **kind:5's OWN `Inserted` row** (always —
kind:5 is an accepted event), but does **NOT** fabricate a `Deleted` row for the already-absent
target (it removed nothing). The mirror's correctness comes from processing that kind:5
`Inserted` row **itself**. This is internally consistent with R2.2: "kind:5 emits an `Inserted`
for the kind:5 event plus one `Deleted` per **actually-removed** target" — an absent target is
not an actually-removed target, so the only row is the kind:5 `Inserted`. No new store behavior
is required; the fix is entirely in the **mirror** contract (the consumer applies kind:5 / NIP-40
to its own copy). The store taxonomy, hook list, and retention rules from Rev 2 / Rev 3 are
**unchanged**.

### R4.4 Parity / contract tests

- **Step-1 (store parity):** the delete-races-eviction case — log a target (`Inserted`),
  retention-evict it (`ByRelayOnly` or LRU, no `Deleted` row), then deliver a kind:5 for it;
  assert the store emits **no** `Deleted` row for the absent target but the log **still** carries
  the kind:5's own `Inserted` row.
- **Step-5 (mirror contract):** on that same sequence, assert the mirror **deletes its copy** of
  the target by processing the kind:5 `Inserted` row itself.

### Untouched (closed in Rev 2 / Rev 3, confirmed still closed by Rev 4)

The `LruEviction` false-delete trap (R3.1), `delete_by_filter` destructive-hook coverage
(R3.2), the ladder step-1 bounded log GC (R3.3 / R2.4), and R2.1 / R2.5 / R2.7 / R2.8 are
**unchanged** by Rev 4. Rev 4 touches only the **mirror-side** delete contract (§5, step-5) and
the framing of `Deleted` rows (§3, R2.2, §6); it adds no store behavior and removes none.

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
     | Deleted { target_id, reason: DeleteReason },   // SEMANTIC removal only — R2.2 / Rev 3
  event_id,
  raw_event: Option<RawEvent>,      // present for Inserted / Replaced (bounded by R2.4)
  source_relay: Option<RelayUrl>,
  received_at_ms,
}

// Rev 3 — `Deleted` rows are SEMANTIC removals a mirror acts on (deletes its copy):
// author delete, NIP-40 expiry, or operator purge. RETENTION removals (LRU eviction,
// delete_by_filter(ByRelayOnly)) emit NO row — the event still validly exists.
DeleteReason = Nip09 | Nip40Expiry | AdminPurge
```

- `Inserted` for accepted stored events; `Replaced` carries the new event + the
  superseded id (matches existing replaceable handling); NIP-09 emits the kind:5
  `Inserted` plus one `Deleted { _, Nip09 }` per removed target. **These rows are
  appended at the store-internal mutation site, NOT derived from the public
  `InsertOutcome`** — kind:5 removes targets as internal side effects and still
  returns `InsertOutcome::Inserted` (Rev 2 R2.2; `mem/insert_kind5.rs:34-38,88-90`).
- **`AdminPurge`** rows come from `delete_by_filter` (`events.rs:363`), one per removed
  primary row, for the destructive `ByAuthor`/`ByIds`/`ByKindRange` variants (R2.2).
- **Retention removals emit no row (Rev 3):** LRU eviction and
  `delete_by_filter(ByRelayOnly)` produce **no** `Deleted` row — even though `ByRelayOnly`
  destroys the primary event row, the event still validly exists elsewhere. A mirror **never**
  deletes its local copy on a retention eviction (R2.2).
- **A mirror is a semantic superset and applies deletes itself (Rev 4):** the mirror keeps
  events the store retention-evicted, so its delete correctness comes from **processing the
  kind:5 `Inserted` row and the `expiration` tag itself**, not from the store's `Deleted` rows
  (which are a best-effort optimization for events the store still held). See §5.
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

**The mirror is a SEMANTIC SUPERSET, not an exact current-store replica (Rev 4).**
Retention removals destroy the producing store's primary row but emit no `Deleted` row:
LRU eviction drops the row, and `delete_by_filter(ByRelayOnly)` **does** delete the primary
event row (`crates/nmp-store/src/mem/insert.rs:147` builds `ids_to_remove`;
`crates/nmp-store/src/lmdb/delete.rs` `by_relay_only`; asserted at
`crates/nmp-store/src/lmdb/tests_kind5.rs:186-218`) — it is destructive, not
provenance-only. A durable mirror **keeps** events the producing store retention-evicted (it
does not re-fetch to re-confirm them), so by construction the mirror's set is a superset of the
store's current authoritative set, not a mirror image of it. **Because it is a superset, the
mirror MUST apply Nostr deletion semantics ITSELF and MUST NOT rely on the store's `Deleted`
rows for correctness:**

- **NIP-09.** The mirror receives every kind:5 event as an ordinary `Inserted` log row (kind:5
  is itself an accepted, stored event — R2.2). The mirror applies that kind:5 to **its own
  copy** — deleting the referenced target ids from its own store — exactly as any Nostr client
  does, **regardless of whether the producing store still held the target.** This is what
  closes the race in which a kind:5 arrives after the target was already retention-evicted: the
  producing store removes nothing and emits no `Deleted` row, but the mirror still acts because
  it processes the kind:5 `Inserted` row itself.
- **NIP-40 expiry.** The mirror applies expiration **locally** from the `expiration` tag on the
  events it holds (it has the events and their tags), regardless of whether the producing store
  already expired and pruned them.

**`Deleted` rows are reframed as a best-effort signal/optimization, NOT the mirror's source of
truth (Rev 4).** A `Deleted { Nip09 | Nip40Expiry | AdminPurge }` row is emitted only for an
event the producing store **still held** at delete time (a removal actually happened). It lets
a consumer that only cares about the store's current authoritative set short-circuit — such a
consumer MAY use `Deleted` rows directly. But a durable superset mirror **MUST additionally
apply NIP-09 / NIP-40 itself** from the log stream / its own copy, because a delete that races
retention eviction produces no `Deleted` row. The retention-class rule from Rev 3 still holds in
full: a mirror **NEVER** deletes on a retention eviction (there is no row to act on, and the
event still validly exists). `AdminPurge` remains a best-effort operator-purge signal a mirror
MAY honor; it has no NIP equivalent to re-derive, so it is advisory only.

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

**Retention eviction is mirror-invisible (Rev 3).** Pruning a *log* row (this GC) or
evicting a *primary* event row (LRU, or `delete_by_filter(ByRelayOnly)`) **never** emits a
consumer-visible `Deleted` row — those events still validly exist. A `Protected` cursor's
floor pin keeps its **unconsumed** log rows from being pruned; a cursor that falls past
`max_lag_entries` gets an explicit `PullGap`, not a false delete. A mirror **never** deletes its
copy on a retention eviction. It deletes its copy by applying NIP-09 / NIP-40 **itself** from the
log stream / its own copy (Rev 4, §5); the store's semantic `Deleted` rows
(`Nip09` / `Nip40Expiry` / `AdminPurge` — R2.2) are a best-effort optimization for events the
store still held, not the mirror's source of truth.

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

> **Re-scope (Rev 2, sharpened in Rev 3).** Step-1 is `GlobalLog`-only and ships the
> **bounded seq-keyed log GC inside the same PR** — the `log_retention_floor_seq` floor
> mechanism *and* a default retention bound both land in step-1, so raw-event duplication
> is never unbounded (there is **no** separate "log GC = later step"). Step-1 appends log
> rows at the store-internal mutation sites (not from `InsertOutcome`) and allocates seq
> inside the event txn. The only retention work deferred is the **advanced
> `Protected`-cursor floor-pin** (`min_protected_cursor_seq` ceiling on the step-1 floor),
> which composes in step-4. `load_older` is **not** solved by step-1 and gets its own
> separate-design step (step-6).

1. **Store ingest-seq index + log (`GlobalLog`) + `scan_log_since_seq` + bounded log GC**
   on both backends (Mem + LMDB). Seq allocated **inside the event mutation txn** (R2.5);
   log rows appended at **store-internal mutation sites** — each kind:5 target removal and
   each `delete_by_filter` removed primary row (R2.2); `op` taxonomy with
   `DeleteReason = Nip09 | Nip40Expiry | AdminPurge` — **LRU eviction and
   `delete_by_filter(ByRelayOnly)` emit NO row** (R2.2 / Rev 3); `PullPage` carries
   `next_after_seq` / `latest_seq` / `has_more` (R2.1); **the seq-keyed log GC with a
   default retention bound ships HERE** (`log_retention_floor_seq` + a default
   `max_log_entries` / size budget), so raw duplication is bounded even with no cursor
   registered (R2.4). Parity tests: late old event after cursor; duplicate → no seq;
   `Replaced` op; NIP-09 `Deleted` ops (per target); `Nip40Expiry` op; **`AdminPurge` op
   per `delete_by_filter` removed row (`ByAuthor`/`ByIds`/`ByKindRange`)**; **`ByRelayOnly`
   and LRU eviction emit NO `Deleted` row**; **delete-races-eviction (Rev 4): a target id is
   logged (`Inserted`), then retention-evicted (`ByRelayOnly` or LRU — no `Deleted` row), then
   a kind:5 for it arrives — assert the store removes nothing for the absent target and emits
   NO `Deleted` row for it, but the LOG still carries the kind:5's own `Inserted` row so a
   superset mirror can act on it**; default log-GC bound prunes the tail and
   `oldest_available_seq()` rises; LMDB reopen seq continuity; no-backfill on upgrade
   (R2.6). *Smallest independently-valuable PR; the keystone.*
2. **Kernel pull service** over the `GlobalLog` scope (no FFI yet). The
   level-triggered drain + initial-wake / re-wake registry semantics (R2.1) live here.
3. **FFI `PullPage` + typed `nmp.pull.wake`** (generalize the #1520 wake). Hard FFI
   page-size cap; document `pull_page` must not run on the UI thread from `apply()`
   (R2.8).
4. **Cursor registration + retention modes** — `GapAllowed` explicit-gap contract +
   `Protected { max_lag_entries }` publishing `min_protected_cursor_seq` as a **ceiling on
   the step-1 `log_retention_floor_seq`** (the advanced retention mode — pin the log floor
   to the slowest protected cursor; degrade to the gap contract past `max_lag_entries`),
   with explicit gap / lag-degrade tests. (The log GC *bound* itself already shipped in
   step-1; this step only adds the Protected floor-pin on top of it.)
5. **`hl` mirror migration** onto the `GlobalLog` cursor (mirror contract = distinct
   events + optional merged-provenance read, not traffic replay). **The mirror is a SEMANTIC
   SUPERSET, not an exact current-store replica (Rev 4): it keeps events the store
   retention-evicts, so its delete path is "apply NIP-09 + NIP-40 from the log stream / its own
   copy," not "act on the store's `Deleted` rows."** Concretely: the mirror processes each
   kind:5 `Inserted` row by deleting that kind:5's target ids from **its own** store, and
   applies NIP-40 expiry from the `expiration` tag on the events it holds — **regardless of
   whether the producing store still held the target.** **Invariant: a delete that races
   retention eviction is still applied by the mirror, because the mirror processes the kind:5
   event itself** (the store removed nothing for the already-absent target and emitted no
   `Deleted` row). The store's `Deleted` rows (`Nip09` / `Nip40Expiry` / `AdminPurge`, R2.2)
   are a best-effort optimization a consumer that only tracks the store's current authoritative
   set MAY use; a durable superset mirror MUST additionally apply NIP-09 / NIP-40 itself. **The
   mirror NEVER deletes on a retention eviction** (no row, event still validly exists, R3.1).
   Mirror-contract test: assert the mirror deletes its copy on the delete-races-eviction case
   (step-1 parity case) by processing the kind:5 `Inserted` row itself. Initial full sync
   (R2.6): **register the cursor at the current head, then export the current store, then pull
   from that cursor** — tolerating duplicates but **never** gaps. Then confirm the #1552 native
   sink stays deleted.
6. **`load_older` → interest-scoped pagination — SEPARATE DESIGN (R2.7).** *Not* a
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
