# ADR-0045 — Store→projection replay (offline / second-launch render)

- Status: Implemented. The single always-on cache-serve seam is live at
  `crates/nmp-core/src/kernel/cache_serve/`; store-served and relay-delivered
  events converge on the shared `project_accepted_event` seam. Owner corrections
  (Revision 2: 2026-06-12, Revision 3: 2026-06-17) are in
  [0045-store-projection-replay-revisions.md](0045-store-projection-replay-revisions.md).
- Date: 2026-06-12
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

Introduce a **store-replay seam invoked at interest-open / compile time**,
budgeted and chunked on the actor tick (never a blocking whole-store scan),
that queries the local store for the newest-N events matching a newly-opened
interest's shape and feeds them through the **existing projection-update
functions** (not `insert`), marked with local provenance. The watermark
rewrite stays, now guarded by the replay-coverage invariant (§6).

Concretely, for each interest the planner newly compiles
(`CompileTrigger::ViewOpened`, and the seed-timeline open):

1. Map the interest `InterestShape` → one or more `StoreQuery` variants (§3).
2. `query_visit` the store newest-first up to a per-shape `replay_limit`
   matching the projection's visible window (§4), on a **budgeted slice** of
   the actor tick (§5) — never a single unbounded scan.
3. For each visited `StoredEvent`, call the **projection-update path** the
   live ingest uses *after* a successful insert — for the timeline:
   `insert_timeline_id_sorted` + populate the `events` read-cache; for observer-
   backed projections: `notify_event_observers` with a `Provenance::LocalStore`
   marker. **Skip `store.insert` entirely.**
4. Mark the interest's replay as completed so re-compiles (reconnect, follow-
   list change) do not re-replay the same shape every tick.

Replayed events are dedup-safe against the *subsequent* live delivery: when a
relay later re-sends a replayed event (e.g. a follow-list change widens the
shape and the relay's `since` floor lets it through), `store.insert` returns
`Duplicate` and the live path's `Duplicate` arm is a no-op — the event is
already in the timeline/cache from replay. The two paths converge on the store
as the single source of truth (D4).

## 3. Filter → store-query mapping

`InterestShape` carries `authors: BTreeSet`, `kinds: BTreeSet`, `event_ids`,
`addresses`, `since/until/limit`, plus tag routing. The store query surface is
the five `StoreQuery` variants (`nmp-store/src/types/query.rs:14`), each backed
by an existing secondary index. Stage-1 mapping (no new index required):

| Shape pattern | `StoreQuery` | Index | Coverage |
|---|---|---|---|
| ≥1 author + ≥1 kind | `AuthorKind` per author, merge newest-first | `idx_author_kind` | timeline, profile feeds |
| 0 authors + ≥1 kind | `KindTime` | `idx_kind_time` | hashtag/global feeds |
| `#e` target + kinds | `Etag` | `idx_etag_time` | thread replies |
| `#p` target + kinds | `Ptag` | `idx_ptag_time` | **DM inbox (kind:1059 `#p`=me)**, mentions |
| addressable coord | `KindDtag` | `idx_kind_dtag_time` | long-form, lists |

Every stage-1 shape maps onto an existing index — **no new index is needed for
stage 1.** The multi-author timeline case issues one `AuthorKind` scan per
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
`[…, newest-stored]` are guaranteed to be replayed into the projection at
open time. Stage 1 satisfies this for the shapes it covers (timeline, DM
inbox); shapes **not** yet covered by replay must **not** be watermark-floored
until their replay lands. This is enforceable as a doctrine-lint / unit
assertion: every shape pattern in the watermark `WatermarkFn` (§1.1,
`kernel/mod.rs:1876`) must have a corresponding replay `StoreQuery` mapping
(§3). The watermark and replay tables are the same table read two ways.

The author-aware watermark fix (#1087/#1091) already narrowed the floor to
per-author `AuthorKind` shapes — which is exactly the stage-1 replay coverage
set. The two changes interlock: #1091 made the floor *safe per author*, #1086
makes the floor *render-complete*.

## 7. Marmot / DM special cases

- **DM inbox (NIP-17 gift-wraps, kind:1059):** the inbox interest is a
  `#p = me` + kind:1059 shape → `StoreQuery::Ptag` (§3). Replayed kind:1059
  events **must go through the same decrypt-on-ingest seam** the live path
  uses (the `EventIngestParser` / DM-inbox projection that unwraps the
  gift-wrap — the cold-start path #1080 wired). Replay feeds the stored
  *ciphertext* kind:1059 through that decrypt seam, not a separate path —
  one decrypt code path (no fragmentation). **Stage 2** scopes this, because
  it needs the decrypt seam to be replay-callable (verify it does not assume
  live-only provenance).
- **MLS group state (Marmot):** MLS ratchet/group state is **not** event-replay
  reconstructable the same way — it is a stateful protocol object, not a
  projection over events. Replaying kind:445/44x events into MLS would corrupt
  ratchet state. **MLS group-message replay is explicitly OUT of scope** for
  this ADR; group *membership/metadata* projections that are pure event
  projections may be covered in a later stage, but the MLS state machine is
  rehydrated by its own persistence path (Marmot storage), not by this seam.

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
itself).** Rejected for stage 1 as the *primary* mechanism — it fragments the
replay policy across every projection (N copies of the budget/limit/ordering
logic), violating the one-canonical-path rule. The chosen design centralizes
replay at the interest-open seam and feeds projections through their existing
update functions. (A narrow opt-in *hook* for projections whose shape does not
map to a `StoreQuery` — should any arise — remains available as an extension,
but is not the stage-1 path.)

**(e) Remove the watermark rewrite (serve everything from relays each launch).**
Rejected (§1.1): unbounded bandwidth, defeats the store-is-authoritative bet,
and still renders empty offline.

## 9. One mechanism — engineering increments (supersedes the staged-by-domain rollout)

> **Superseded by Revision 2 (2026-06-12).** The original §9 staged the rollout
> *by domain* — timeline (stage 1), then DMs (stage 2), then generalize (stage 3)
> — and gated when cache-serve "turned on" per domain. The owner correction
> (R2.1) rejects that: cache-serve is **one always-on mechanism** that runs for
> every opened interest from the first launch, not a feature that lands
> domain-by-domain. This section now describes the *single seam* and the
> *engineering increments* by which it is implemented — increments of one
> mechanism, never product stages.

**The mechanism (one seam, always on).** At interest-open / compile
(`CompileTrigger::ViewOpened` and the seed-timeline open), **every**
`LogicalInterest` — any shape, any consumer — is served from the local store
first: map `InterestShape` → `StoreQuery` (§3), `query_visit` newest-first under
the per-tick budget (§4–§5), and feed each `StoredEvent` through the post-store
projection-dispatch seam with `Provenance::LocalStore` (§1.2), **never**
`store.insert` (§8(c)). The planner's wire REQ is the refinement half of the same
mechanism. This runs unconditionally — cold/warm/offline/online — with offline
being the degenerate case where the wire half delivers nothing.

**Universal acceptance test (the contract):** launch twice, **second launch
offline**, and **every** open interest's projection renders from the store —
feed, DM inbox, threads, long-form, and anything else open. (Rev 1's per-stage
"exit" criteria collapse into this one universal test.)

**Engineering increments (of one mechanism — not product stages).** Landing may
proceed incrementally for delivery hygiene; each increment is the *same* seam
applied to more shapes / tuned, gated by the universal test above, not a
per-domain on-switch:

- **E1 — Land the seam + first shapes.** Add `Provenance::LocalStore`, the
  budgeted store-serve step at interest-open/`ViewOpened`, the post-store
  projection-dispatch feed, the one-shot per-(interest, shape) completion marker,
  and the `AuthorKind` + `KindTime` shape mappings (timeline / profile / global).
  The seam is general from the first commit — these are simply the first shapes
  wired through it.
- **E2 — DM ciphertext through the shared decrypt seam.** Verify the #1080
  decrypt seam is provenance-agnostic and route `Ptag` kind:1059 store-serve
  through it (the same seam that unwraps live gift-wraps — one decrypt path,
  R2.4(f)). This is *the same seam decrypting store-served ciphertext*, not a new
  "DM stage."
- **E3 — Remaining shapes + structural invariant guard.** Wire `Etag` (threads),
  `KindDtag` (addressable / long-form), `Ptag` mentions, and any observer-backed
  projection whose shape maps to a `StoreQuery`. Land the §6 assertion as a
  **structural seam-identity check** (every floored shape passes through the one
  cache-serve seam — by construction, since the seam is universal), not a
  per-shape coverage table. MLS group state explicitly excluded (§7, R2.4(g)).

These increments may overlap or land together; their only purpose is incremental
delivery of **one** seam. There is no point at which cache-serve is "on for the
feed but off for DMs" as a shipped state — the universal acceptance test forbids
it.

## 10. Consequences

- D1 honored: offline/second-launch renders from the store, refines from relays.
- The watermark rewrite becomes the *complete* NDK pattern (floor + cache-serve),
  guarded by an enforceable invariant.
- One new provenance marker; zero new indexes for stages 1–2; reuse of the
  existing bounded timeline cache and projection-update functions.
- The replay budget adds bounded per-tick work; profiling gates any index
  optimization (post-v1).

## 11. v1 vs post-v1 — DECIDED by owner (2026-06-12): universal cache-serve gates v1

> **Decided by owner (2026-06-12).** The original question — "do stages 1–2 gate
> v1?" — assumed a domain-staged rollout that no longer exists. Under the one
> always-on mechanism the question is "does universal cache-serve gate v1?" and
> the owner has **decided: yes.** This is no longer a recommendation.

**Decision: universal cache-serve gates v1.** The single always-on mechanism,
passing its **universal acceptance test**, is a **v1 exit criterion**:

> **v1 exit criterion (cache-serve):** launch twice; the second launch offline;
> **every open interest's projection renders from the store** — feed, DM inbox,
> threads, long-form, and anything else the v1 apps open. Offline-empty for any
> open, store-backed interest blocks v1.

`docs/plan.md` carries this in its v1 blocker list (issue #1086,
`phase:v1-blocker`, `priority:p1`).

Under the single-mechanism design there is no meaningful "feed gates v1 but DMs
are post-v1" line to draw — cache-serve is one seam that either renders every
open interest from the store or it does not. A half-shipped "feed offline-renders
but DMs do not" state is the very split the owner rejected (R2.1) and is not a
coherent v1 cut.

Rationale (why the owner gated v1 on it):

- The owner ships iOS + Android + desktop (web is out of v1). On all three,
  "reopen the app — especially offline — and see your feed, your DMs, the thread
  you were reading" is table-stakes app behavior, not a refinement.
- An app that renders empty on second launch despite a full local store fails the
  most basic user expectation and directly contradicts the framework's own D1
  doctrine — and aim.md §4.1's "every read goes through the store" — that the
  product is sold on.
- Because the mechanism is *one seam*, a half-shipped state ("feed offline-renders
  but DMs do not") is not a coherent v1 cut; it is the very split the owner
  rejected. The honest v1 line is "the one mechanism is in, and the universal
  acceptance test passes for the shapes the v1 apps open."

Engineering nuance: the seam itself (E1) and the shapes the v1 apps actually open
(timeline + DM at minimum; threads / long-form per app) are what the universal
test must cover at the v1 cut. Shapes no v1 app opens (e.g. a projection only a
future app would declare) do not block v1 in practice — not because cache-serve is
"off" for them, but because no v1 view exercises them; the seam still serves them
the moment such a view is opened. The structural invariant guard (E3) can land
alongside or just after, since under the single seam it is a seam-identity
assertion, not a per-shape coverage gate.
