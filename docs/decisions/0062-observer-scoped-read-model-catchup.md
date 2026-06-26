# ADR-0062 — Observer-scoped read-model catch-up: delivery ≠ population

- **Status:** Accepted (2026-06-26)
- **Date:** 2026-06-21
- **Issues:** #1645 (delete redundant explicit LMDB hydration in
  `open_author_feed`), #1646 (the thread-root half). This ADR is the prerequisite
  that makes both safe to land — without it, deleting the seeds is a silent
  regression (proven empirically; see Context).
- **Decision record:** this ADR is the durable, self-contained authority for the
  observer-activation / read-model catch-up architecture.
- **Extends ADR-0045** (single always-on cache-serve mechanism) and **ADR-0057**
  (unified ingest chokepoint). Neither changes: ADR-0045's "one acquisition path,
  replay feeds the post-store seam not `store.insert`" stands, and ADR-0057's
  global accept-once chokepoint stands. This ADR adds a **delivery** invariant
  *beside* them; it does not touch acquisition or population.
- **Related:** ADR-0042 §5.1 (`register_feed_with_observer` per-open feeds),
  ADR-0053 (host-declared projections — observers self-gate by registration),
  `docs/wiki/guides/store-first-interest-registration.md`,
  `docs/wiki/guides/lmdb-event-store.md`.
- **API naming update (#2089, historical mapping):** the API names below are
  preserved as written at decision time. In current code: the live-event tap
  registrar method `register_event_observer` is now `register_live_event_tap`
  (trait `EventObserverRegistrar` → `LiveEventTapRegistrar`), and the
  `register_feed_with_observer` per-open feed seam has been **removed** — its
  muted→activate-with-read-cache-replay role is now the
  `ObservedProjectionRegistrar::open_observed_projection` /
  `close_observed_projection` door (the canonical realization of this ADR's
  delivery invariant). Read the names in this document as those current APIs.

---

## Context

### The defect: population and delivery are fused

The kernel has exactly **one** event-acquisition mechanism (ADR-0045 cache-serve):
when an interest opens, `Kernel::register_interest`
(`crates/nmp-core/src/kernel/cache_serve/mod.rs:210`) synchronously drains a chunk
that scans the LMDB store and feeds matching events through the **same** post-store
dispatch the live-ingest path uses:
`feed_served_event` → `project_accepted_event` → `notify_event_observers`.

Two facts combine into a hole:

1. **Cache-serve dedups against the in-memory read-cache.** `serve_chunk`
   (`cache_serve/continuation.rs:99`) skips any candidate already present in
   `self.events`: `if !events_cache.contains_key(&ev.raw.id) { collected.push(...) }`.
   This dedup is **load-bearing on three counts** and must stay:
   - (a) `feed_served_event` (`continuation.rs:202`) inserts into `self.events`,
     bumps metrics, runs the NIP-parser + capability-transition sweep, and appends
     the timeline — re-feeding a cached event double-counts and double-fires the
     **global** observer broadcast.
   - (b) it guards the live↔serve race for the #1520 post-completion wakeup
     (`ingest/accepted.rs:130`).
   - (c) the inclusive-`until` cursor-resume in `serve_chunk` (`continuation.rs:149`)
     deliberately re-visits boundary-timestamp events and relies on the dedup to
     swallow them.

2. **A live tap is a one-shot broadcast, while an observed projection must be
   interest-scoped.** The live-tap seam (`register_live_event_tap`) replays
   nothing to a newly-registered observer and intentionally has no interest
   shape. The observed-projection seam
   (`ObservedProjectionRegistrar::open_observed_projection`) carries the filter
   and replay shapes required for kernel-owned catch-up and scoped live delivery.

**Consequence.** An observer registered *after* an interest's read-model is warm —
which a per-open feed (Chirp author/thread profile, `interest_feed.rs`) **always**
is — receives none of the matching events already in `self.events`: cache-serve
skips them (cached), and there is no registration replay. The broadcast that would
have delivered them already fired before the observer existed.

The kernel is using **"event X is in the read-model"** as a proxy for
**"every observer has received event X."** Those are different facts; the proxy
holds only for observers registered before ingest (the startup-registered
home-feed / timeline projections) and is false for every late joiner.

### Empirical proof

FFI actor-ordering tests over the real kernel:

| Order | Result |
|---|---|
| open feed **then** inject (observer present for the live broadcast) | feed populates |
| inject **then** open feed (relying on cache-serve alone) | feed stays empty |

The only difference is whether matching events were already cached when the
interest opened — isolating the failure to the cache-serve dedup + missing replay.

### Why the app currently "works"

The Chirp shell papers over the hole with `seed_author_feed_from_store` /
`seed_thread_feed_from_store` (`apps/chirp/crates/nmp-app-chirp/src/ffi/interest_feed.rs:344,363`),
which read LMDB **directly** and feed the `FlatFeed`. Issues #1645/#1646 correctly
identify this as an anti-pattern (the kernel, not the app, should own event
acquisition — `store-first-interest-registration.md:37`) — but the seeds are
**load-bearing** until the kernel owns the catch-up. Deleting them first is the
regression this ADR prevents.

### This is a general kernel defect, not a Chirp quirk

The same late-join class affects any observer/projection registered after its
interest's first drain:
- **FollowListProjection** — the doc's own follow-list bug
  (`store-first-interest-registration.md:38-39`): registered after its kind:3 was
  already cache-served, so it missed the broadcast. The doc's workaround (re-push
  the interest) is fragile: a completion key already in `served_interest_shapes`
  makes `enqueue_cache_serve` a no-op (`cache_serve/mod.rs:303`).
- **DM-inbox / `IngestParser`s** registered after a gift-wrap was already served.
- **Anything registered after an account switch / `Reset`.**

One kernel-owned invariant replaces three app-side hacks.

### `self.events` lifecycle (why both replay *and* cache-serve are needed)

`self.events` is a bounded LRU, HWM = `EVENTS_RAM_HWM = 1000`
(`crates/nmp-core/src/kernel/ram_eviction.rs:122`), evicted oldest-`created_at`-first
on the 60 s GC tick; open interests pin their matches via `open_view_pins`
(`ram_eviction.rs:218-242`). So a freshly-opened feed's matches split into:
- a **cached head** (in `self.events`) — *missed today*, fixed by the new replay;
- an **evicted tail** (LMDB-only) — already served correctly by the existing
  cache-serve drain through the global notify.

The dedup partitions the two cleanly: cached ⇒ replay; evicted ⇒ serve.

---

## Decision

### Invariant

> **Any observed projection activated for an interest receives every matching
> event already present in the read-model — cached (in-memory) or stored (LMDB) —
> exactly once, no event twice, and only matching future live events.**

Population stays a global, accept-once concern (cache-serve / ADR-0057 chokepoint).
**Delivery to a late-joining observer becomes a separate, observer-scoped,
read-only replay.** Apps MUST NOT read the store directly to hydrate observers.

### Mechanism — register muted, then open → replay → activate scoped

A new **activation protocol** makes observer mounting and interest-open atomic and
correctly ordered by construction. The activation transition is the single
hand-off point between *replay (catch-up)* and *live (broadcast)*.

1. `ObservedProjectionRegistrar::open_observed_projection` registers the observer
   **muted** — present in the slot, excluded from `notify_event_observers`.
2. The app issues one atomic
   `ActorCommand::OpenObservedInterest { filter_json, consumer_id, scope, relay_pin, observer_id, replay_shapes }`
   (parsed like `OpenInterest`), replacing any separate "register observer +
   push `open_interest`" pair.
3. Kernel runs `register_interest` unchanged — cache-serve populates `self.events`
   and notifies the **existing** (active) observers; the muted new observer is not
   notified.
4. Kernel replays the matching `self.events` to **only** that observer id.
5. Kernel calls `activate_observer_scoped` with the registered interest shape —
   it now receives only future events matching that observed projection. The
   legacy `activate_observer` function remains for explicit live taps that need
   unfiltered all-event delivery.

Because the observer is muted through steps 3–4, no live event can reach it before
the replay, so the replay cannot duplicate a live delivery; and because replay runs
on every observed-open **activation** (not gated on `register_interest`'s
`changed` flag), it also covers the **multi-owner** case where `EnsureAbsent`
attaches a second owner to an already-open `(scope,key)` slot with `changed:false`
(`crates/nmp-core/src/subs/registry.rs:104`) — there the new observer is empty and
still needs hydration.

### Seam (kernel-internal)

```rust
// Observer slot (crates/nmp-core/src/actor/commands/event_observer.rs)
pub fn register_rust_observer_muted(
    slot: &KernelEventObserverSlot,
    observer: Arc<dyn KernelEventObserver>,
) -> KernelEventObserverId;
pub(crate) fn notify_observer_by_id(
    slot: &KernelEventObserverSlot, id: KernelEventObserverId, event: &KernelEvent,
);
pub fn activate_observer_scoped(
    slot: &KernelEventObserverSlot,
    id: KernelEventObserverId,
    shape: InterestShape,
) -> bool;

// Kernel (crates/nmp-core/src/kernel/...)
pub(crate) struct ObserverReplayRequest {
    pub observer_id: KernelEventObserverId,
    pub shapes: Vec<InterestShape>,   // plural — see thread-root below
    pub limit: usize,
}
pub(crate) fn open_interest_with_observer_replay(
    &mut self,
    identity: SubIdentity,
    interest: LogicalInterest,
    replay: ObserverReplayRequest,
    reason: &'static str,
) -> RegistrationOutcome;
```

Replay body — a **read-cache snapshot**, never `feed_served_event`:

```rust
fn replay_read_cache_to_observer(&self, req: &ObserverReplayRequest) {
    let now = self.now_secs();
    let mut rows = self.events.values()
        .filter(|ev| req.shapes.iter().any(|s|
            s.matches_event_with_id(&ev.id, &ev.author, ev.kind, ev.created_at, &ev.tags)))
        .cloned().collect::<Vec<_>>();
    rows.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
    for ev in rows.into_iter().take(req.limit) {
        let mut ke = kernel_event_from_stored_cache(ev);
        ke.created_at = ke.created_at.min(now);          // D9 clamp (projection.rs:84)
        self.notify_observer_by_id(req.observer_id, &ke);
    }
}
```

- The shape filter reuses `InterestShape::matches_event_with_id`
  (`crates/nmp-planner/src/interest.rs:439`) — the **same** predicate
  `open_view_pins` and admission use, so the catch-up filter and the eviction-pin
  filter cannot drift.
- **D9 future-date clamp is mandatory** (`ingest/projection.rs:84`): `self.events`
  holds the raw `created_at`, so a hostile future-dated event must be clamped at
  replay time or it pins to the top of the feed.
- Replay **order is irrelevant** to correctness: `FlatFeed` is id-keyed on insert
  (`crates/nmp-nip01/src/flat_feed.rs:138`) and sorts at snapshot time (`:164`).
  Sorting here is for the `limit` cap, not for the consumer.

### What stays unchanged

- The cache-serve dedup (`continuation.rs:99`) — load-bearing on the three counts
  above.
- The single acquisition mechanism (ADR-0045) and the accept-once chokepoint
  (ADR-0057).
- The global broadcast for live + evicted-tail events.
- `feed_served_event`'s read-model writes, metrics, parser dispatch, the #1520
  wakeup (post-completion invalidation — orthogonal).

The replay **must not** mutate `self.events`, metrics, parser caches,
`served_interest_shapes`, or store wakeups — it is pure delivery.

---

## Alternatives rejected

- **Remove the dedup / re-serve cached events globally.** Reintroduces the
  double-count, re-fires the parser + transition sweep, and re-notifies every
  observer — the exact single-fire violation the dedup exists to prevent.
- **Replay hook on `register_event_observer`.** The observer has no shape; there
  is nothing to filter `self.events` against. Rejected as structurally impossible
  without the interest context.
- **Thread `observer_id` into `register_interest` and replay there, gated on
  `changed`.** Misses the multi-owner `changed:false` case, and leaves a
  live↔replay double-delivery window because the observer is globally active
  before the open command runs. The mute→replay→activate protocol closes both.
- **Make cache-serve generally observer-aware.** Turns every `PendingCacheServe`
  into a per-observer delivery state machine; over-couples acquisition to
  delivery. Rejected in favour of a small replay seam *beside* cache-serve.

---

## Migration

1. Land the activation seam (muted registration, `OpenObservedInterest`,
   `replay_read_cache_to_observer`).
2. Port Chirp per-open feeds to `OpenObservedInterest` and **delete**
   `seed_author_feed_from_store` (#1645) and the reply half of
   `seed_thread_feed_from_store` (#1646).
3. **Thread-root caveat (#1646):** `shape_to_store_queries` intentionally returns
   no store scan for `event_ids`-only shapes (`cache_serve/queries.rs:147`). The
   replay covers a **cached** root via the `{"ids":[root]}` member of
   `replay_shapes`; an **evicted** root still needs #1646's bounded `ids=[...]`
   store front-door. So thread open replays two shapes:
   `{"#e":[root],"kinds":[1,6]}` (replies) and `{"ids":[root]}` (root).
4. Retire the FollowListProjection startup workaround
   (`store-first-interest-registration.md:38-39`) once the catch-up is
   kernel-owned; reconcile the two wiki guides (the LMDB-replay "hack" guidance vs
   the "observer must replay on registration" requirement) to point at this ADR.

## Consequences

- **Positive:** one kernel invariant replaces three app-side hacks; per-open feeds
  populate correctly regardless of cache warmth; D0 stays clean (the kernel names
  only `KernelEventObserverId` + `InterestShape`); the seeds (#1645/#1646) become
  safe to delete.
- **Cost:** a real FFI-surface change — observer mute/active state + a new combined
  command — not a local patch. This is precisely why the issues' "just delete the
  seeds" framing was unsafe.
- **Risk:** the muted→active transition must be race-free against `Reset` /
  account switch (`clear_served_interest_shapes`, `cache_serve/mod.rs:395`) and
  against an observer dropped mid-activation; covered by activating strictly after
  replay on the actor thread (single-writer, D4).
