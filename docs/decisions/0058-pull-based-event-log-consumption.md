# ADR-0058 — Cursor-based event-log consumption (the "pull" model)

- **Status:** Accepted pending implementation (design ratified; PR ladder queued — see §8)
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
event data — so the cursor is **edge-triggered, not polled** (D8). The durable store
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
  op:  Inserted | Replaced { replaced_id } | Deleted { target_id, reason },
  event_id,
  raw_event: Option<RawEvent>,      // present for Inserted / Replaced
  source_relay: Option<RelayUrl>,
  received_at_ms,
}
```

- `Inserted` for accepted stored events; `Replaced` carries the new event + the
  superseded id (matches existing replaceable handling); NIP-09 emits the kind:5
  `Inserted` plus one `Deleted` per removed target (matches existing delete paths).
- **Duplicates do not create log entries** — see §5.
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
  `nmp-ingest-meta` (`last_seq`). **Seq allocation happens inside the same write txn
  as the event mutation** (D4: one writer, atomic — the store already commits NMP
  sub-dbs with event writes in one txn).

## 5. Duplicates / provenance

The pull primitive delivers **distinct stored events, once** — not per-arrival
duplicates. Rationale:

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

Production runs LRU eviction (`GcBudget::production`, `HOT_EVENT_CEILING = 10_000`),
so a slow consumer's cursor can point past evicted log rows. Two modes:

- **`GapAllowed`**: if `after_seq < oldest_available_seq - 1`, the page returns
  `PullGap { requested_after_seq, first_available_seq }` — an explicit gap, **never a
  silent skip**.
- **`Protected { max_lag_entries }`**: an actor-owned cursor registration adds a
  **bounded retention claim** (`protected_log_floor_seq`) threaded through the
  **existing GC pin input** (the same caller-derived pins/coverage the store already
  takes — not a parallel retention loop). If lag exceeds the bound, the cursor
  degrades to the explicit gap contract.

Because `Inserted`/`Replaced` log rows carry the raw event bytes, the **primary
event rows need not be pinned for mirror correctness** — only the log floor. UI rows
stay protected by existing interest/feed claims. This ties into the K3
coverage-ledger pin machinery (ADR-0056), it does not duplicate it.

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
- **The exact anti-pattern ADR-0039 named is avoided**: the cursor is paired with the
  #1520 **wake** signal, so it is edge-triggered. It supplies precisely the
  change-signal ADR-0039 said pull lacked. No `Task.sleep` poll loop is introduced.

"Pull" here therefore names a distinct concept from ADR-0039's rejected
"projection pull accessor"; the two ADRs are complementary, not in tension.

## 7. Doctrine alignment

- **D1** reads-through-store — the cursor reads the durable store, no side fetch.
- **D4** single writer — the ingest-seq is allocated in the event's own write txn;
  the log has one writer (the ingest chokepoint, ADR-0057).
- **D5** bounded — pull is intrinsically backpressured; `max_pending`-style push
  state machines are deleted. No speculative per-arrival log.
- **D8** no polling — the wake signal makes consumption edge-triggered.

## 8. Implementation ladder (the remaining work)

The store **foundation** overlaps #1523 and is partly landed. The remaining,
pull-specific work is tracked in **#1566** (a #1523 subissue):

1. **Store ingest-seq index + log + `scan_log_since_seq`** on both backends
   (Mem + LMDB), with parity tests: late old event after cursor; duplicate → no
   seq; `Replaced` op; NIP-09 `Deleted` ops; LMDB reopen seq continuity.
   *Smallest independently-valuable PR; the keystone.*
2. **Kernel pull service** over `GlobalLog` + `InterestShape` scopes (no FFI yet).
3. **FFI `PullPage` + typed `nmp.pull.wake`** (generalize the #1520 wake).
4. **Retention/GC cursor registration** (gap contract + protected-mode log-floor
   pin) with explicit gap tests.
5. **`hl` mirror migration** onto the pull cursor; then confirm the #1552 native
   sink stays deleted.
6. **`load_older` → `PullCursor`** wrapper (UI "give me more" rides the same
   substrate; feed display order unchanged).

## 9. Risk

Bigger in **store** work (seq allocation, multi-op logging, LMDB schema migration,
GC log-floor) — but **simpler and safer at runtime**: no retain-until-ack
dispatcher, no hollow batches, no `created_at` watermark holes, no push-side
backpressure state machine. Crash recovery is "resume from seq." The hard part moves
to the correct layer — the durable store.
