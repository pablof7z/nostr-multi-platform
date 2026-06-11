# ADR-0045 — Store→projection replay (offline / second-launch render)

- Status: Accepted pending implementation
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

- **`replay_limit` per interest = the projection's visible window**, defaulting
  to `min(shape.limit.unwrap_or(DEFAULT), TIMELINE_CACHE_LIMIT)`. The store is
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

## 9. Staged rollout

**Stage 1 — Timeline replay (minimal falsifiable slice).**
Scope: the host-declared follow-feed timeline interest only (`AuthorKind` +
`KindTime` shapes). Add `Provenance::LocalStore`; add the budgeted replay step
at the seed-timeline open / `ViewOpened` compile; feed
`insert_timeline_id_sorted` + `events` cache directly; one-shot completion
marker; replay budget per tick. Assert the watermark⇄replay invariant for
timeline shapes. **Exit:** launch twice; second launch online shows the full
pre-watermark feed (not just post-watermark events); unit test proves replay
feeds the timeline without `store.insert`.

**Stage 2 — DM inbox replay (offline acceptance gate).**
Scope: NIP-17 gift-wrap inbox (`Ptag` kind:1059), replayed ciphertext through
the existing decrypt seam (#1080). Extend the invariant to DM shapes.
**Exit (the issue's falsifiable criterion):** launch twice, **second launch
offline**, feed **and** DM inbox both non-empty. This is the headline
acceptance test of the whole effort.

**Stage 3 — Generalize + invariant lint.**
Scope: thread (`Etag`), addressable/long-form (`KindDtag`), mentions (`Ptag`),
and any observer-backed projection whose shape maps to a `StoreQuery`. Land the
doctrine-lint/unit assertion enforcing §6 (every watermark-floored shape has a
replay mapping). **Exit:** the watermark⇄replay table is enforced in CI; no
floored shape lacks replay coverage. MLS group state explicitly excluded.

## 10. Consequences

- D1 honored: offline/second-launch renders from the store, refines from relays.
- The watermark rewrite becomes the *complete* NDK pattern (floor + cache-serve),
  guarded by an enforceable invariant.
- One new provenance marker; zero new indexes for stages 1–2; reuse of the
  existing bounded timeline cache and projection-update functions.
- The replay budget adds bounded per-tick work; profiling gates any index
  optimization (post-v1).

## 11. v1 vs post-v1 (owner decides)

**Recommendation: stages 1–2 gate v1; stage 3 is early-post-v1.**

Rationale: the owner ships iOS + Android + desktop (web is out of v1). On all
three, "reopen the app and see your feed / your DMs" — especially offline — is
table-stakes app behavior, not a refinement. An app that renders empty on
second launch despite a full local store fails the most basic user expectation
and directly contradicts the framework's own D1 doctrine that the product is
sold on. Stages 1–2 are the minimum that makes the store-is-authoritative bet
true for the user, and stage 2 is the issue's own falsifiable acceptance test.
Stage 3 (thread/long-form/mention generalization + the lint) hardens coverage
but no single view in it is launch-blocking the way feed+DM are; it can land in
the first post-v1 hardening pass. `docs/plan.md` should carry a one-line pointer
(this is `priority:p1`); the owner adjudicates the final v1 line.
