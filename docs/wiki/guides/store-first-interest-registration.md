---
title: Store-First Interest Registration (Enqueue and Drain)
slug: store-first-interest-registration
topic: data-persistence
summary: "ADR-0045 establishes a single event-acquisition mechanism: at interest-open time the store is scanned (cache-serve), and relay delivery fills the tail"
tags:
  - capture
volatility: warm
confidence: high
created: 2026-06-18
updated: 2026-06-26
verified: 2026-06-26
compiled-from: conversation
sources:
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:e6b44a84-8cfc-48b2-863a-58382398b5df
---

# Store-First Interest Registration (Enqueue and Drain)

## One Event-Acquisition Path

ADR-0045 establishes a single event-acquisition mechanism: at interest-open time the store is scanned (cache-serve), and relay delivery fills the tail. Both paths feed events through the **same** post-store projection dispatch (`insert_timeline_id_sorted` → read-cache → `notify_event_observers`). Store-served events do NOT re-enter `store.insert` — the `Duplicate` arm would skip timeline append and observer fan-out.

A locally published kind:3 event must reflect immediately in the UI regardless of relay connectivity, as a natural consequence of the offline-first architecture where locally stored signed events are indistinguishable from relay-received events.

Etag and Ptag StoreQuery variants carry no time cursor (since/until); this is intentional conservative over-serve, with relay filling the tail. The `shape_to_store_queries` function maps all ADR-0045 E1–E3 `InterestShape` patterns correctly to `StoreQuery` variants; uncovered cases (wildcard kinds, multi-tag intersection, event-ids-only, unrecognized tag keys, text/search) are intentional and documented.

<!-- citations: [^129d2-46] [^129d2-47] [^129d2-66] [^129d2-118] [^e6b44-8] -->
## Enqueue

`Kernel::register_interest` (called when a view opens) calls `enqueue_cache_serve` to push a `PendingCacheServe` onto the back of the queue. `enqueue_cache_serve` never scans — it only queues. Dedup: re-registering the same interest scope+shape does nothing if a completion key already exists in `served_interest_shapes`.

The first chunk of the first interest is drained **synchronously** before `register_interest` returns. This ensures data is in the read-cache before the next snapshot frame, eliminating the "blank first frame" UX problem.

Explicit cache-warming (hydrating an observer by replaying LMDB data at registration time) is a hack; the correct architecture is demand-driven interest registration where a projection pushes its own interest when it is used, causing the cache-serve to flow through naturally. (Previously: the comment in `register.rs:392` stated 'no separate interest push is needed — events arrive through the standing subscription'.)

The follow list display bug (profiles showing 'Follow' for already-followed users and the follow button staying stuck) is caused by an ordering issue: startup registers and cache-serves the kind:3 interest before the FollowListProjection observer is registered, so the observer misses the initial cache-serve event. The fix is that `nmp_app_chirp_register_follow_list` must push its own kind:3 interest for the active account (via `PushInterest` or `EnsureInterest`) immediately after registering the observer, rather than relying on the standing subscription from startup.

The #1516 streaming `query_visit` implementation preserves tie-group buffering for equal `created_at` events, delivering `(created_at DESC, id ASC)` ordering consistent with the existing contract. A `#[cfg(test)]` `AtomicUsize` conversion counter verifies streaming `query_visit` does not over-materialize; the test `streaming_visit_does_not_over_materialize` inserts 1000 events, breaks at 10, and asserts ≤11 conversions. The early-stop materialization regression gate test in #1524 is marked `#[ignore]` until #1516's streaming `query_visit` lands, because it would fail against the current pre-materialization code.

The old `open_contact_feed` / `follow_feed_kinds` path is retired. Active-user
follows now compile through the generic ReducedSource/dependent-interest feed
source, so sign-in, account switch, and follow-list replacement re-run source
reduction instead of preserving a parallel kernel follow-feed registry.

<!-- citations: [^129d2-67] [^11850-179] [^e6b44-9] -->
## Drain (Budget-Bounded)

`Kernel::run_cache_serve_step` drains the queue on each actor tick (≤250 ms wake, same cadence as GC). It operates under a **shared aggregate budget**:

```
cache_serve_tick_budget = 2 × visible_limit (events visited per tick)
```

`visible_limit` is the consumer's render window (default 80 for timeline). The budget is in *store events visited*, not events served — a non-matching visited event still consumes budget. This bounds actor work per tick regardless of how many interests are queued.

If a `PendingCacheServe` exceeds the tick budget, it saves a `until` cursor (the last-visited `created_at`) and resumes on the next tick. No new timers are needed: the actor loop's existing idle-tick piggybacks the drain (D8 — no polling).

## Serve Depth

Each interest is served at most `min(shape.limit, visible_limit)` events (`Kernel::serve_depth_for_shape`). A tailing follow feed has no relay-wire limit (`None`), so its depth is just `visible_limit` — the window cannot show more anyway.

Once the timeline holds ≥ `visible_limit` entries, subsequent timeline-bound queries are floored at the window-edge `created_at`. Events scanned below that floor cannot enter the visible window → the scan early-stops (`ControlFlow::Break`).

## Completion Keys

When a serve finishes (possibly multiple ticks later), its completion key (scope-key + shape-content hash) is recorded in `served_interest_shapes`. Re-compiles (relay disconnect, follow-list change) do NOT re-serve a completed shape. `Kernel::clear_served_interest_shapes` drops both the queue and completion set on account-switch.

A post-serve live event may invalidate a completed shape. The cache-serve wakeup mechanism (#1520, PR #1541) uses a `BTreeSet<u64>` coalescing buffer (not a channel, not a timer) on the Kernel. It fires `note_store_insert` from the live-ingest chokepoint after `project_accepted_event`. `note_store_insert` only inserts into `cache_serve_wakeups` when the interest is already in `served_interest_shapes` (completed); pending interests are handled by their register-time serve, preventing double-serving. It walks active `(SubKey, LogicalInterest)` pairs, checks `matches_event_with_id`, and inserts matching keys into the buffer. `drain_cache_serve_wakeups` then removes each woken key from `served_interest_shapes`, looks up the interest in the registry, silently drops stale keys for closed views, and re-enqueues via the existing deferred path. The buffer drains on the existing actor idle tick, complying with D8 (no polling, no timers, no unbounded channels).

<!-- citations: [^129d2-72] [^129d2-95] [^129d2-119] -->
## Dedup Safety

- Serve → live: a relay re-delivers a store-served event → `store.insert` sees `Duplicate`, no observer fan-out.
- Live → serve: events already in the read-cache are skipped at visit time in `feed_served_event`.

`feed_served_event` calls `project_accepted_event` → `notify_event_observers`,
so cache-served events do reach declared observers. Late-opening views must use
observed projections because the public raw event-observer lane is not a
supported app surface.

<!-- citations: [^e6b44-10] -->
