# ADR-0058 — Cursor-based event-log consumption (the "pull" model)

- **Status:** Accepted pending implementation (design ratified; PR ladder queued — see §8).
  Hardened over revisions 2–4 (2026-06-18) against adversarial pre-build review; the body
  (§1–§9) carries the final decided contract. Inline `(Rev N / R2.x)` tags mark provenance;
  see the concise **Revision history** at the end.
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

## Revision history

The body above (§1–§9) is the final decided contract. Three adversarial pre-build codex
reviews (all 2026-06-18) hardened it; their decisions were folded into the body inline. The
process, condensed:

- **Rev 2 (pre-implementation hardening).** Made the wake contract **level-triggered**
  (`PullPage` carries `next_after_seq` / `latest_seq` / `has_more`; consumer drains, never
  sleep-rechecks; initial wake + re-wake while lag remains) — the airtight ADR-0039
  reconciliation (§3, §6.1). The mutation log is appended at **store-internal mutation
  sites** under the same lock/txn, **not** derived from `InsertOutcome` (kind:5 removes
  targets as side effects — §3, §4). Seq is allocated **inside the event write txn** (LMDB
  `RwTxn` / Mem mutex), **never** an `AtomicU64` (§4, §7). Corrected the false
  "production runs LRU at `HOT_EVENT_CEILING`" premise; retention is a **separate seq-keyed
  log GC** and the bounded log GC ships **in step-1** so raw-event duplication is never
  unbounded (§6, §8). Split `load_older` out — step-1 `scan_log_since_seq` is `GlobalLog`-only;
  interest-scoped feed pagination is a separate design (§3, §8 step-6). Promoted FFI page
  caps + no-UI-thread-`pull_page` to MUST (§3, §8 step-3).
- **Rev 3 (residual hardening).** `Deleted` rows are **semantic removals only**: dropped
  `LruEviction` from `DeleteReason` entirely (acting on it would destroy valid data) —
  `DeleteReason = Nip09 | Nip40Expiry | AdminPurge` (§3, §5). Added `delete_by_filter` to the
  hook list: destructive `ByAuthor`/`ByIds`/`ByKindRange` → `AdminPurge`; `ByRelayOnly` is
  retention-class → **no** row (§5, §8). Resolved the ladder contradiction: the bounded log GC
  ships **in** step-1, only the `Protected`-cursor floor-pin defers to step-4 (§8).
- **Rev 4 (mirror semantic-delete contract).** A semantic delete (NIP-09/NIP-40) racing a
  retention eviction removes nothing in the store and emits no `Deleted` row. So a durable
  mirror is a **semantic superset** that **applies NIP-09 (from each kind:5 `Inserted` row) and
  NIP-40 (from held `expiration` tags) itself**; store `Deleted` rows are a best-effort
  optimization for events the store still held, not the mirror's source of truth; the mirror
  **never** deletes on retention eviction. The store emits the kind:5's own `Inserted` row but
  no `Deleted` row for an already-absent target (§3, §5, §6, §8 step-5).
