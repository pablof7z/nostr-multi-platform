# ADR-0045 — Store→projection replay (offline / second-launch render)

- Status: Implemented. The single always-on cache-serve seam is live at
  `crates/nmp-core/src/kernel/cache_serve/`; store-served and relay-delivered
  events converge on the shared `project_accepted_event` seam.
- Date: 2026-06-12 (folded owner corrections 2026-06-12 and 2026-06-17)
- Issue: #1086 (F-12), `doctrine:d1`, `priority:p1`, `area:core`, `area:store`
- Related: #1087/#1091 (author-aware watermark rewrite — the load-bearing
  precondition this ADR completes), #1085/V-117 (gc_step unbudgeted scan —
  the anti-precedent for actor-thread store scans), #617 (constructor blocks
  on synchronous LMDB open — the anti-precedent for whole-store replay at
  construction), #1080 (F-02 cold-start DM receive).

## 1. Context

NMP persists every accepted event to LMDB (`EventStore::insert`, D4
single-writer). But **no code path replays stored events into projections.**
Projections (timeline, DM inbox, group chat, zap aggregate) are fed
*exclusively* by live relay deliveries through one funnel:

```
relay frame → handle_event (kernel/ingest/mod.rs:368)
            → store.insert (dedup)
            → IF Inserted|Replaced: timeline append + observer/projection fan-out
            → IF Duplicate: relay_count bump ONLY — no append, no fan-out
              (kernel/ingest/timeline.rs:117-130; mod.rs:476 V-40 gate)
```

Meanwhile the T129 watermark rewrite (`subs/recompile.rs:291-310`,
`kernel/mod.rs:1860-1909`) floors every non-ephemeral sub-shape's `since` at
`newest-stored-for-that-shape + 1`, so **relays never re-send what is already
on disk.** The two facts compose into a structural D1 violation:

- **Second launch (online):** the feed shows only events created *after* the
  last session's newest-stored timestamp. Everything already on disk is
  watermark-suppressed at the relay and never re-enters a projection.
- **Second launch (offline):** there are no relay deliveries at all. The feed
  and DM inbox render **empty despite a full local store.**

This contradicts D1 ("render now, refine in place") — the same doctrine
#606/#607 invoke against iOS render gates — and it is the framework's biggest
credibility gap versus what external consumers will hit first (podcast-player's
offline episode lists; any app reopened on a plane).

### 1.1 Why the watermark is not the bug — and why it cannot be reverted

The watermark rewrite is *correct NDK heritage* (`since_rewrite_tests.rs:6`,
`addSinceFromCache`). NDK floors subscriptions at newest-cached **because NDK
also serves the subscription from cache first** (`core/src/subscription/
index.ts:537` — "once-at-sub-open" cache emit). NMP copied the floor (the
bandwidth-saving half) without the cache-serve half (the render half). Removing
the watermark would re-fetch the entire store from relays on every launch —
unbounded bandwidth, violates the bet that the store is authoritative, and
still does nothing offline. **The fix is to add the missing half, not remove
the present one.** This ADR makes the watermark *safe* by guaranteeing replay
coverage for every shape whose `since` it floors (the invariant in §6).

### 1.2 The load-bearing constraint: replay cannot go through `insert`

The obvious design — "re-feed stored events through `handle_event` with
local provenance" — **does not work**, and verifying this is the central
finding of this ADR. A stored event re-entering `store.insert` returns
`InsertOutcome::Duplicate` (it is already on disk), and the `Duplicate` arm
(`kernel/ingest/timeline.rs:117-130`) deliberately does **not** append to the
timeline or fan to observers — it only bumps `relay_count`. The V-40 fan-out
gate (`kernel/ingest/mod.rs:476`) is `Inserted | Replaced` only, precisely so
sibling-relay re-deliveries do not double-count. Replay through `insert` would
therefore be a silent no-op for every event already stored — i.e. for
*exactly* the events replay exists to surface.

**Conclusion:** replay must feed the *projection-update* seam directly (the
post-store half: `insert_timeline_id_sorted` + `events` read-cache +
`notify_event_observers`), treating the store as the authority it already is,
**not** re-route through the insert-dedup gate. This is the inverse of the live
path: live = insert-then-project (dedup at insert); replay = project-from-store
(dedup at insert is irrelevant — the store already accepted it once).

## 2. Decision

There is exactly **one** event-acquisition mechanism, and serving from the local
store is the **first half** of it; the planner's wire REQ is the **refinement
half**. Both halves run together, unconditionally, for **every** opened/compiled
`LogicalInterest` — any shape, any consumer, host-declared or built-in, consumer
feed or the active account's own bootstrap kinds (kind:0 profile, kind:3 contacts,
kind:10002 relay list, kind:10000/10006). This is a **single always-on cache-serve
seam**, not an offline mode and not a per-domain feature:

- **One seam, every interest, store-first.** On open / compile
  (`CompileTrigger::ViewOpened`, the seed-timeline open, the bootstrap and
  one-shot discovery interests in `kernel/requests/startup.rs`), the interest is
  served from the local store immediately through `enqueue_interest_cache_serve`,
  and its wire REQ fires in parallel to revalidate and tail. Time-to-first-pixel
  is zero; the network keeps the view current from there.
- **Always on; offline is the degenerate case.** Cold/warm/offline/online, the
  store-serve half runs. There is no "offline path" and no "online path" — one
  path with two halves. Offline rendering is simply the case where the wire half
  delivers nothing; the on-disk copy *is* the current copy and rendering it
  immediately is correct (aim.md §4.1, "every read goes through the store").
- **Literally the same code for cached and fresh.** A store-served event and a
  relay-delivered event flow through the *same* `project_accepted_event` seam
  under the *same* supersession rule (newest `created_at` wins; event-id
  tiebreak), differing **only** by `Provenance::LocalStore` vs relay provenance.
  When the relay returns an event we already hold, supersession makes it a no-op;
  when it returns a newer one — or another client signs one mid-session — the same
  seam re-drives every downstream effect. Serving a stored kind:3 updates the
  active-account source state, the ReducedSource owner replaces its materialized
  author interests through the generic dependent-interest path, and the followed
  authors' notes are served with the app doing nothing. A later kind:3 signed in
  another client routes through the identical seam and recompiles the feed under
  the new follow set.

For each interest the cache-serve seam:

1. Maps the interest `InterestShape` → one or more `StoreQuery` variants (§3).
2. `query_visit`es the store newest-first up to a per-shape limit matching the
   projection's visible window (§4), on a **budgeted slice** of the actor tick
   (§5) — never a single unbounded scan, never a blocking scan at construction.
3. Feeds each visited `StoredEvent` through the **post-store projection-dispatch
   seam** the live ingest uses *after* a successful insert (`project_accepted_event`
   → `insert_timeline_id_sorted` + `events` read-cache + `notify_event_observers`),
   marked `Provenance::LocalStore`. It **skips `store.insert` entirely** (§1.2),
   because a re-fed stored event returns `InsertOutcome::Duplicate` whose arm is a
   deliberate no-op.
4. Marks the interest's serve completed so re-compiles (reconnect, follow-list
   change) do not re-serve the same shape every tick.

Store-served events are dedup-safe against the *subsequent* live delivery: when a
relay later re-sends one, `store.insert` returns `Duplicate` and the live path's
`Duplicate` arm is a no-op — the event is already in the timeline/cache. Both
paths converge on the store as the single source of truth (D4).

The watermark rewrite stays, now holding **universally by construction** (§6):
because cache-serve is part of the one mechanism every floored shape passes
through, "no watermark floor without cache-serve for the same shape" is true for
all shapes — including the bootstrap kinds — automatically, making the §6 check a
structural seam-identity guard rather than a per-shape coverage table.

This is a **return** to aim.md §4.1, which already promised a single store-first
read path with a network fallback half — the store-serve seam is the read half,
the wire REQ is the in-kernel realization of the fallback half.

## 3. Filter → store-query mapping

`InterestShape` carries `authors: BTreeSet`, `kinds: BTreeSet`, `event_ids`,
`addresses`, `since/until/limit`, plus tag routing. The store query surface is
the five `StoreQuery` variants (`nmp-store/src/types/query.rs:14`), each backed
by an existing secondary index. The mapping (no new index required):

| Shape pattern | `StoreQuery` | Index | Coverage |
|---|---|---|---|
| ≥1 author + ≥1 kind | `AuthorKind` per author, merge newest-first | `idx_author_kind` | timeline, profile feeds |
| 0 authors + ≥1 kind | `KindTime` | `idx_kind_time` | hashtag/global feeds |
| `#e` target + kinds | `Etag` | `idx_etag_time` | thread replies |
| `#p` target + kinds | `Ptag` | `idx_ptag_time` | **DM inbox (kind:1059 `#p`=me)**, mentions |
| addressable coord | `KindDtag` | `idx_kind_dtag_time` | long-form, lists |

Every shape maps onto an existing index — **no new index is needed.** The
multi-author timeline case issues one `AuthorKind` scan per
author and merges by `created_at` newest-first under the global `replay_limit`
(§4); this mirrors the per-author watermark scan #1091 already does, so the
index access pattern is proven. A future optimization (a follow-set-aware
merged index) is explicitly out of scope and tracked as a post-v1 follow-up if
profiling shows the per-author fan-out is a bottleneck.

## 4. Ordering and limits (D8 bounded working set)

Projections expect roughly chronological ingest with a bounded working set.
The timeline read-cache is already bounded — `TIMELINE_CACHE_LIMIT = 500`
(`kernel/ingest/timeline_order.rs:5`), newest-first via
`insert_timeline_id_sorted`. Replay policy:

> **Decided by owner (2026-06-12) — serve depth default = 1× the view's visible
> window.** At interest-open, cache-serve delivers the **newest-N events matching
> the consumer's visible window** (e.g. a timeline serves ≈ its cache limit, not
> the whole store). Older history arrives via the **normal network refinement /
> scroll paths**, not via a deeper initial replay. Deeper-than-1× serve is a
> **per-interest opt-in** if a consumer ever needs it, **never** the default.
> This keeps the serve half bounded to exactly what a live session of the same
> view would hold (D8) and avoids re-materializing arbitrarily deep history on
> every open.

- **`replay_limit` per interest = the projection's visible window** (the decided
  1× default above), defaulting to
  `min(shape.limit.unwrap_or(DEFAULT), TIMELINE_CACHE_LIMIT)`. The store is
  scanned newest-first and `query_visit` early-stops (`ControlFlow::Break`) at
  the limit — no full-store materialization.
- Replay feeds events **oldest-of-the-window first** into
  `insert_timeline_id_sorted` so the sorted-insert cost stays cheap and the
  final order is identical to live ingest (the function is order-independent —
  it sorts on insert — but feeding ascending keeps each insert near the tail).
- The 500-cap means replay can never exceed the live working-set bound. Replay
  does not grow memory beyond what a live session of the same view would hold.

## 5. Actor-thread budget (the #1085 anti-precedent)

`gc_step` (V-117 / #1085) is the cautionary tale: it does **unbudgeted
O(store) scans on the actor thread**, blocking the reducer. Replay must not
repeat that mistake. Constraints:

- Replay runs **inside the existing actor tick**, never on a spawned polling
  task (D8: no polling) and never as a blocking whole-store scan at construction
  (#617: the constructor already blocks too long on LMDB open — replay must not
  pile on).
- Each tick processes a **bounded slice**: at most `REPLAY_BUDGET_EVENTS` per
  interest per tick (chunked continuation across ticks for large shapes), so a
  single 500-event replay across many newly-opened interests does not stall the
  first snapshot. The continuation cursor is the `until` bound of the next
  `query_visit` (newest-first paging by `created_at`).
- The first snapshot after launch should reflect *at least the first replay
  slice* of the active view, so the user sees content immediately and the rest
  refines in place (D1: render now, refine in place).
- Replay is **idempotent and one-shot per (interest, shape)**: a completion
  marker on the interest prevents re-replay on every reconnect/recompile.

This keeps replay O(visible-window), bounded per tick, and off the blocking
construction path — structurally avoiding the V-117 class of bug.

## 6. The watermark ⇄ replay invariant

> **No watermark floor without replay coverage for the same shape.**

The watermark rewrite (`apply_watermark_rewrite`) may floor a sub-shape's
`since` at `newest-stored + 1` **only if** that shape's stored events
`[…, newest-stored]` are guaranteed to be cache-served into the projection at
open time. Because cache-serve is the one always-on seam every floored shape
passes through, this holds by construction for all shapes — including the
bootstrap kinds. It remains expressible as a doctrine-lint / unit assertion:
every shape pattern in the watermark `WatermarkFn` (§1.1, `kernel/mod.rs:1876`)
has a corresponding cache-serve `StoreQuery` mapping (§3). The watermark and
cache-serve tables are the same table read two ways.

The author-aware watermark fix (#1087/#1091) narrowed the floor to per-author
`AuthorKind` shapes: #1091 made the floor *safe per author*, #1086 makes the
floor *render-complete*.

## 7. Marmot / DM special cases

- **DM inbox (NIP-17 gift-wraps, kind:1059):** the inbox interest is a
  `#p = me` + kind:1059 shape → `StoreQuery::Ptag` (§3). Replayed kind:1059
  events **must go through the same decrypt-on-ingest seam** the live path
  uses (the `EventIngestParser` / DM-inbox projection that unwraps the
  gift-wrap — the cold-start path #1080 wired). Cache-serve feeds the stored
  *ciphertext* kind:1059 through that decrypt seam, not a separate path —
  one decrypt code path (no fragmentation), as a property of the uniform path
  (the decrypt seam is provenance-agnostic).
- **MLS group state (Marmot):** MLS ratchet/group state is **not** event-replay
  reconstructable the same way — it is a stateful protocol object, not a
  projection over events. Replaying kind:445/44x events into MLS would corrupt
  ratchet state. **MLS group-message replay is explicitly OUT of scope** for
  this ADR; group *membership/metadata* projections that are pure event
  projections ride the one cache-serve seam like any other shape, but the MLS
  state machine is rehydrated by its own persistence path (Marmot storage), not
  by this seam.

## 8. Alternatives considered

**(a) Replay at interest-open / compile time, feeding the projection-update
seam directly — CHOSEN.** Bounded to the visible window, reuses the existing
post-insert projection functions, no new index, naturally scoped to what the
user opened. Cost: requires a `Provenance::LocalStore` marker and a budgeted
tick slice. Verified feasible against the existing `insert_timeline_id_sorted`
(timeline_order.rs:8) and `notify_event_observers` (ingest/mod.rs:443) seams.

**(b) Whole-store replay at kernel construction.** Rejected. Violates D5/D8
(unbounded working set — the store can hold far more than any view shows) and
#617 (the constructor *already* blocks too long on synchronous LMDB open;
replaying the whole store there would delay the first snapshot by seconds on a
large store). It also replays events for views the user never opens.

**(c) Re-route stored events through `handle_event` / `store.insert` with
local provenance.** Rejected — and this is the subtle one. Verified at
`kernel/ingest/timeline.rs:117-130` + `kernel/ingest/mod.rs:476`: a stored
event re-entering `insert` returns `Duplicate`, and the `Duplicate` arm is a
deliberate no-op (no timeline append, no fan-out). Replay through insert is a
silent no-op for exactly the events it must surface. The dedup gate that makes
live ingest correct makes insert-based replay impossible.

**(d) Per-projection store-hydration opt-in (each projection queries the store
itself).** Rejected as the *primary* mechanism — it fragments the cache-serve
policy across every projection (N copies of the budget/limit/ordering logic),
violating the one-canonical-path rule. The chosen design centralizes cache-serve
at the interest-open seam and feeds projections through their existing update
functions. (A narrow opt-in *hook* for projections whose shape does not map to a
`StoreQuery` — should any arise — remains available as an extension.)

**(e) Remove the watermark rewrite (serve everything from relays each launch).**
Rejected (§1.1): unbounded bandwidth, defeats the store-is-authoritative bet,
and still renders empty offline.

## 9. Acceptance test (the contract)

> **Launch twice; the second launch offline. EVERY open interest's projection
> renders from the store** — feed, DM inbox, threads, long-form, the bootstrap
> kinds, and anything else open. Offline-empty for any open, store-backed interest
> is a failure of the one mechanism.

There is no shipped state in which cache-serve is "on for the feed but off for
DMs": one seam either renders every open interest from the store or it does not.

## 10. Consequences

- D1 honored: offline/second-launch renders from the store, refines from relays.
- The watermark rewrite becomes the *complete* NDK pattern (floor + cache-serve),
  guarded by an enforceable invariant.
- One new provenance marker; zero new indexes; reuse of the
  existing bounded timeline cache and projection-update functions.
- The replay budget adds bounded per-tick work; profiling gates any index
  optimization (post-v1).

## 11. v1 exit criterion (owner decision, 2026-06-12)

Universal cache-serve gates v1. The single always-on mechanism, passing its
acceptance test (§9), is a v1 exit criterion:

> **v1 exit criterion (cache-serve):** launch twice; the second launch offline;
> **every open interest's projection renders from the store** — feed, DM inbox,
> threads, long-form, and anything else the v1 apps open. Offline-empty for any
> open, store-backed interest blocks v1.

`docs/plan.md` carries this in its v1 blocker list (issue #1086,
`phase:v1-blocker`, `priority:p1`). Because the mechanism is one seam, there is no
coherent "feed gates v1 but DMs are post-v1" line — it either renders every open
interest from the store or it does not. This is table-stakes app behavior on the
v1 platforms (iOS + Android + desktop; web is out of v1): reopen the app, even
offline, and see your feed, your DMs, the thread you were reading.
