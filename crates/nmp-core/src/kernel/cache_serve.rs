//! ADR-0045 E1 — Store-cache serve seam (chunked continuation).
//!
//! The **first half** of the one event-acquisition mechanism: at
//! interest-open time, map the `InterestShape` → `StoreQuery` variants, scan
//! the store newest-first, and feed results through the **same post-store
//! projection-dispatch path** relay-delivered events take
//! (`insert_timeline_id_sorted` + `events` read-cache +
//! `notify_event_observers`) — NOT through `store.insert`, whose `Duplicate`
//! arm deliberately skips timeline append and observer fan-out (ADR §1.2).
//!
//! ## Aggregate budget — chunked continuation (ADR §5, the #1085 lesson)
//!
//! `gc_step` (V-117 / #1085) did unbudgeted O(store) scans on the actor
//! thread. The follow feed registers ONE single-author interest PER followed
//! pubkey, so a per-interest budget alone is insufficient: a 300–500-follow
//! cold start would still burst `follows × budget` events synchronously.
//! Cache-serve therefore budgets at the **aggregate** level:
//!
//! - [`Kernel::enqueue_cache_serve`] only queues work — it never scans.
//! - [`Kernel::run_cache_serve_step`] drains the queue under ONE shared
//!   per-tick budget ([`Kernel::cache_serve_tick_budget`], counted in
//!   store-events *visited* — visits are the actor work, served or not).
//! - Work that exceeds the tick budget stays queued, with a per-query
//!   `until` cursor, and resumes on the next actor tick. The actor loop
//!   piggybacks `run_cache_serve_step` on its existing ≤250 ms wake
//!   (same pattern as the #1069 gc tick — no new timers, D8).
//!
//! ## Serve depth = 1× the consumer's visible window (ADR §4, owner-decided)
//!
//! Each interest is served at most `min(shape.limit, visible_limit)` events
//! ([`Kernel::serve_depth_for_shape`]). `shape.limit` is the relay-wire
//! backfill cap (e.g. the follow feed's `Some(1000)`), NOT the render
//! window — the kernel's `visible_limit` (= the consumer's visible window,
//! `DEFAULT_VISIBLE_LIMIT = 80` for the timeline) caps it because the
//! snapshot cannot show more anyway.
//!
//! For the per-follow case the WINDOW is the **feed's**, not per-author:
//! the feed needs ~window newest events across ALL follows, not
//! `follows × window`. Chosen mechanism (documented per review): newest-N
//! per author, with an aggregate-window `since` floor — once the timeline
//! already holds ≥ `visible_limit` entries, every subsequent timeline-bound
//! query is floored at the window-edge `created_at`, so authors whose
//! stored events cannot enter the visible window early-stop in the index
//! scan. Events served before the floor rose stay in the timeline
//! (bounded by `TIMELINE_CACHE_LIMIT`); the final visible window is exactly
//! the newest-W superset regardless of serve order.
//!
//! ## Dedup safety
//!
//! Serve→live: relay re-delivery hits `store.insert` `Duplicate` (no
//! observer fan-out). Live→serve: events already in the read-cache are
//! skipped at visit time. Verified in `cache_serve_tests.rs`.
//!
//! ## Provenance
//!
//! Served events carry `relay_count: 0` — the de-facto
//! `Provenance::LocalStore` marker (no relay confirmed the event this
//! session). ADR-0045 R2.4(b) names an explicit marker; pending that ADR
//! amendment, `relay_count == 0 ⇔ local-store-served` is the encoding.
//!
//! ## Completion marker
//!
//! Each completion key (interest scope-key + shape-content hash) is recorded
//! in `served_interest_shapes` when its serve **finishes** (possibly several
//! ticks after enqueue). Re-compiles (relay reconnect, follow-list change)
//! do NOT re-serve a completed shape. Cleared on account-switch via
//! [`Kernel::clear_served_interest_shapes`] (which also drops queued serves).
//!
//! ## Watermark ⇄ serve invariant (ADR §6)
//!
//! > No watermark floor without cache-serve coverage for the same shape.
//!
//! The `watermark_fn` (kernel/mod.rs) refuses to floor tag-/address-/
//! event-id-filtered shapes precisely because E1 serve does not cover them;
//! `cache_serve_budget_tests::e1_watermark_serve_invariant_shapes_are_aligned`
//! pins the load-bearing implication (floored ⇒ served) against a live
//! kernel, not just the variant mapping.

use super::Kernel;
use crate::planner::InterestShape;
use crate::store::StoreQuery;
use crate::substrate::KernelEvent;

/// One queued (possibly partially-completed) store-cache serve. Owned by
/// `Kernel::pending_cache_serves`; queries are mutated in place to carry the
/// resume cursor (`until` lowered to the last visited `created_at`).
pub(super) struct PendingCacheServe {
    /// One-shot completion key — inserted into `served_interest_shapes` when
    /// this serve finishes (all queries exhausted or depth satisfied).
    pub(super) completion_key: u64,
    /// `StoreQuery` list derived from the interest shape at enqueue time.
    queries: Vec<StoreQuery>,
    /// Index of the query currently being drained.
    query_idx: usize,
    /// Events still to serve for this interest (starts at the consumer's
    /// visible window — see [`Kernel::serve_depth_for_shape`]).
    remaining_depth: usize,
    /// Whether this serve feeds the follow-feed timeline (every shape author
    /// was in `timeline_authors` at enqueue time). Enables the
    /// aggregate-window `since` floor. Stale-flag safe: the flag only gates
    /// an optimization; the per-event `timeline_authors` check at feed time
    /// is the correctness gate.
    timeline_bound: bool,
}

impl Kernel {
    /// Serve depth for one interest: 1× the consumer's visible window.
    ///
    /// `shape.limit` is the relay-wire backfill cap (the follow feed carries
    /// `Some(1000)`); the kernel's `visible_limit` is the consumer's render
    /// window. The serve depth is the smaller of the two — serving past the
    /// visible window is wasted actor work (ADR §4, owner decision
    /// 2026-06-12: depth = 1× visible window).
    fn serve_depth_for_shape(&self, shape: &InterestShape) -> usize {
        let declared = shape.limit.map(|l| l as usize).unwrap_or(usize::MAX);
        declared.min(self.visible_limit).max(1)
    }

    /// Aggregate per-tick serve budget, counted in store events **visited**
    /// (visits are the actor-thread work, whether or not the event is fed).
    ///
    /// Derived from the visible window (2×) rather than a fixed constant so
    /// the bound scales with what one snapshot can surface: by default
    /// `2 × DEFAULT_VISIBLE_LIMIT = 160` visits per tick, shared across ALL
    /// pending serves (ADR §5 — a single replay across many newly-opened
    /// interests must not stall the first snapshot).
    fn cache_serve_tick_budget(&self) -> usize {
        (self.visible_limit * 2).max(1)
    }

    /// Queue a store-cache serve for a newly-installed interest.
    ///
    /// Never scans the store — scanning happens in budgeted chunks via
    /// [`Kernel::run_cache_serve_step`]. Idempotent: a completion key that is
    /// already served or already queued is a no-op. Shapes E1 does not cover
    /// are marked served immediately (no retry, no queue entry).
    pub(in crate::kernel) fn enqueue_cache_serve(
        &mut self,
        shape: &InterestShape,
        completion_key: u64,
    ) {
        if self.served_interest_shapes.contains(&completion_key) {
            return;
        }
        if self
            .pending_cache_serves
            .iter()
            .any(|p| p.completion_key == completion_key)
        {
            return;
        }

        let queries = shape_to_store_queries(shape);
        if queries.is_empty() {
            // Shape not covered by E1 — mark served so we don't re-derive.
            self.served_interest_shapes.insert(completion_key);
            return;
        }

        let timeline_bound = !shape.authors.is_empty()
            && shape
                .authors
                .iter()
                .all(|a| self.timeline_authors.contains(a));

        self.pending_cache_serves.push_back(PendingCacheServe {
            completion_key,
            queries,
            query_idx: 0,
            remaining_depth: self.serve_depth_for_shape(shape),
            timeline_bound,
        });
    }

    /// Whether any cache-serve work is queued. The actor loop gates its
    /// per-tick [`Kernel::run_cache_serve_step`] call on this — an empty
    /// queue costs one bool check per wake (D8: no false-wakeup work).
    #[must_use]
    pub fn has_pending_cache_serves(&self) -> bool {
        !self.pending_cache_serves.is_empty()
    }

    /// Drain queued cache-serves under ONE shared per-tick budget.
    ///
    /// Called from the actor loop (piggybacked on the existing ≤250 ms wake,
    /// like the #1069 gc tick) and once synchronously by the two enqueue
    /// sites (`open_interest_sub`, `sync_follow_feed_interests`) so the
    /// first snapshot after an open carries store data (D1). Work beyond the
    /// budget stays queued with a resume cursor and continues next tick.
    ///
    /// Returns the number of events fed into projections this step.
    pub fn run_cache_serve_step(&mut self) -> usize {
        if self.pending_cache_serves.is_empty() {
            return 0;
        }
        let mut tick_remaining = self.cache_serve_tick_budget();
        let mut total_served = 0usize;

        while tick_remaining > 0 {
            let Some(mut pending) = self.pending_cache_serves.pop_front() else {
                break;
            };
            let finished = self.serve_chunk(&mut pending, &mut tick_remaining, &mut total_served);
            if finished {
                self.served_interest_shapes.insert(pending.completion_key);
            } else {
                // Budget exhausted mid-interest — resume here next tick.
                self.pending_cache_serves.push_front(pending);
                break;
            }
        }

        if total_served > 0 {
            self.changed_since_emit = true;
            self.events_since_last_update = self
                .events_since_last_update
                .saturating_add(total_served as u64);
        }
        total_served
    }

    /// Drain as much of one pending serve as `tick_remaining` allows.
    ///
    /// Returns `true` when the serve is finished (all queries exhausted or
    /// depth satisfied) — the caller then records the completion key.
    fn serve_chunk(
        &mut self,
        pending: &mut PendingCacheServe,
        tick_remaining: &mut usize,
        total_served: &mut usize,
    ) -> bool {
        while pending.query_idx < pending.queries.len() {
            if pending.remaining_depth == 0 {
                return true;
            }
            if *tick_remaining == 0 {
                return false;
            }

            // Aggregate-window floor: once the timeline already holds a full
            // visible window, a timeline-bound query only needs events that
            // would beat the window edge. Computed fresh per chunk — the
            // floor rises as the drain progresses, collapsing late authors'
            // scans to near-zero work. `since` is inclusive so window-edge
            // ties are kept (over-serve is safe; under-serve is not).
            let floor = if pending.timeline_bound && self.timeline.len() >= self.visible_limit {
                self.timeline
                    .get(self.visible_limit - 1)
                    .and_then(|id| self.events.get(id))
                    .map(|e| e.created_at)
            } else {
                None
            };

            let query = &pending.queries[pending.query_idx];
            let mut effective = query.clone();
            if let Some(floor_ts) = floor {
                if let Some(since) = query_since_mut(&mut effective) {
                    *since = Some(since.map_or(floor_ts, |s| s.max(floor_ts)));
                }
            }

            let visit_limit = (*tick_remaining).min(pending.remaining_depth.max(1));
            let prev_until = query_until(query);

            // Phase 1 — collect (immutable borrow of the events cache).
            let mut collected: Vec<CollectedEvent> = Vec::new();
            let mut visited = 0usize;
            let mut last_visited_created_at: Option<u64> = None;
            {
                let store = std::sync::Arc::clone(&self.store);
                let events_cache = &self.events;
                let serve_target = pending.remaining_depth;
                let _ = store.query_visit(&effective, visit_limit, &mut |ev| {
                    visited += 1;
                    last_visited_created_at = Some(ev.raw.created_at);
                    // Live→serve dedup: already reflected in projections.
                    if !events_cache.contains_key(&ev.raw.id) {
                        collected.push(CollectedEvent {
                            id: ev.raw.id.clone(),
                            author: ev.raw.pubkey.clone(),
                            kind: ev.raw.kind,
                            created_at: ev.raw.created_at,
                            tags: ev.raw.tags.clone(),
                            content: ev.raw.content.clone(),
                        });
                        if collected.len() >= serve_target {
                            return std::ops::ControlFlow::Break(());
                        }
                    }
                    std::ops::ControlFlow::Continue(())
                });
            }

            // Budget accounting: visits are the actor work (index walk +
            // filter), so they consume the tick budget even when deduped.
            *tick_remaining = tick_remaining.saturating_sub(visited.max(1).min(*tick_remaining));

            // Phase 2 — feed oldest-first so each insert lands near the tail
            // of the sorted timeline deque (cheaper on average).
            let served = collected.len();
            collected.reverse();
            for ev in collected {
                self.feed_served_event(ev);
            }
            pending.remaining_depth = pending.remaining_depth.saturating_sub(served);
            *total_served += served;

            let exhausted = visited < visit_limit;
            if exhausted {
                // Index has no more matches below the cursor — next query.
                pending.query_idx += 1;
                continue;
            }

            // More events may remain: lower the resume cursor. `until` is
            // inclusive, so boundary-timestamp events are re-visited next
            // chunk and deduped via the events cache.
            let new_until = last_visited_created_at;
            if served == 0 && new_until == prev_until {
                // Pathological tie: a whole chunk of already-served events at
                // one timestamp and the cursor cannot descend. Advance to the
                // next query rather than livelock; any same-timestamp events
                // beyond the visit limit arrive via the relay path instead.
                pending.query_idx += 1;
                continue;
            }
            match (new_until, query_until_mut(&mut pending.queries[pending.query_idx])) {
                (Some(ts), Some(until)) => *until = Some(ts),
                _ => {
                    // Cursor-less query variant (cannot occur for E1 shapes,
                    // D6: degrade instead of panic) — no resume possible, so
                    // advance rather than re-scan the same head next chunk.
                    pending.query_idx += 1;
                    continue;
                }
            }
            // Stay on this query; the outer loop re-checks budget/depth.
        }
        true
    }

    /// Feed one store-served event into the projection-dispatch path — the
    /// same seam relay-delivered events use after `Inserted | Replaced`
    /// (ADR-0045 §2, step 3).
    fn feed_served_event(&mut self, ev: CollectedEvent) {
        let cached = super::types::StoredEvent {
            id: ev.id.clone(),
            author: ev.author.clone(),
            kind: ev.kind,
            created_at: ev.created_at,
            tags: ev.tags.clone(),
            content: ev.content.clone(),
            // De-facto `Provenance::LocalStore` marker (see module docs):
            // no relay has confirmed this event in the current session.
            relay_count: 0,
        };

        // Incremental diagnostic counters — mirrors ingest_timeline_event.
        self.metric_stored_events = self.metric_stored_events.saturating_add(1);
        if ev.kind == 1 {
            self.metric_note_events = self.metric_note_events.saturating_add(1);
        }
        self.events.insert(ev.id.clone(), cached);
        self.cached_estimated_store_bytes.set(None);

        let kernel_event = KernelEvent {
            id: ev.id.clone(),
            author: ev.author.clone(),
            kind: ev.kind,
            created_at: ev.created_at,
            tags: ev.tags,
            content: ev.content,
        };
        self.notify_event_observers(&kernel_event);

        // Append to the timeline only when the author is in the follow set —
        // mirrors the post-insert branch of `ingest_timeline_event`.
        if self.timeline_authors.contains(&ev.author) {
            self.insert_timeline_id_sorted(ev.id);
        }
    }

    /// Clear the served-interest completion set AND the pending serve queue.
    ///
    /// Must be called on account-switch / kernel reset so the next identity's
    /// interests get a fresh serve and the prior identity's queued serves do
    /// not keep draining.
    pub(in crate::kernel) fn clear_served_interest_shapes(&mut self) {
        self.served_interest_shapes.clear();
        self.pending_cache_serves.clear();
    }
}

/// Owned copy of a store event taken inside the `query_visit` visitor.
struct CollectedEvent {
    id: String,
    author: String,
    kind: u32,
    created_at: u64,
    tags: Vec<Vec<String>>,
    content: String,
}

fn query_until(query: &StoreQuery) -> Option<u64> {
    match query {
        StoreQuery::AuthorKind { until, .. } | StoreQuery::KindTime { until, .. } => *until,
        _ => None,
    }
}

/// Mutable access to a query's `until` cursor — `None` for variants without
/// one. E1 only enqueues `AuthorKind`/`KindTime` (see
/// `shape_to_store_queries`); a cursor-less variant degrades gracefully at
/// the call sites (the chunk advances to the next query instead of resuming).
fn query_until_mut(query: &mut StoreQuery) -> Option<&mut Option<u64>> {
    match query {
        StoreQuery::AuthorKind { until, .. } | StoreQuery::KindTime { until, .. } => Some(until),
        _ => None,
    }
}

/// Mutable access to a query's `since` bound — `None` for variants without
/// one (the aggregate-window floor is then simply not applied).
fn query_since_mut(query: &mut StoreQuery) -> Option<&mut Option<u64>> {
    match query {
        StoreQuery::AuthorKind { since, .. } | StoreQuery::KindTime { since, .. } => Some(since),
        _ => None,
    }
}

// ─── shape → StoreQuery mapping (ADR §3, E1 shapes) ─────────────────────────

/// Map an `InterestShape` to the `StoreQuery` variants E1 covers.
///
/// Returns an empty vec when the shape has no E1 mapping. The shapes NOT
/// covered here are:
/// - Wildcard kinds (too broad — no safe bounded index).
/// - Tag-filtered (`#e`, `#p`, addresses, event_ids) — E2/E3.
///
/// E1 coverage:
/// - ≥1 author + ≥1 kind → one `AuthorKind` query per author.
/// - 0 authors + ≥1 kind → `KindTime`.
pub(super) fn shape_to_store_queries(shape: &InterestShape) -> Vec<StoreQuery> {
    // Wildcard kinds: not covered by E1.
    if shape.kinds.is_empty() {
        return Vec::new();
    }

    // Tag-filtered, address-filtered, or event-id-filtered: not covered (E2/E3).
    if !shape.tags.is_empty() || !shape.addresses.is_empty() || !shape.event_ids.is_empty() {
        return Vec::new();
    }

    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();

    if shape.authors.is_empty() {
        // KindTime — global / hashtag feed (0 authors + ≥1 kind).
        vec![StoreQuery::KindTime {
            kinds,
            since: shape.since,
            until: shape.until,
        }]
    } else {
        // AuthorKind — one query per author; results merged under the shared
        // budget. Mirrors the per-author watermark scan `#1091` uses.
        use crate::kernel::hex_to_pubkey_bytes;
        shape
            .authors
            .iter()
            .filter_map(|author_hex| {
                let author = hex_to_pubkey_bytes(author_hex)?;
                Some(StoreQuery::AuthorKind {
                    author,
                    kinds: kinds.clone(),
                    since: shape.since,
                    until: shape.until,
                })
            })
            .collect()
    }
}

/// Derive the completion key for an interest.
///
/// A stable hash of the interest's `SubKey` + the shape's content fields
/// (authors, kinds). `since/until/limit` and routing metadata are excluded —
/// a shape that widens its time window should not retrigger a full re-serve
/// (the watermark+relay refinement handles the delta).
pub(super) fn completion_key_for_interest(
    sub_key: &crate::subs::SubKey,
    shape: &InterestShape,
) -> u64 {
    use crate::stable_hash::stable_hash64;
    let authors: Vec<&str> = shape.authors.iter().map(|s| s.as_str()).collect();
    let kinds: Vec<u32> = shape.kinds.iter().copied().collect();
    stable_hash64((sub_key, &authors, &kinds))
}
