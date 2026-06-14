//! ADR-0045 E1–E3 — Store-cache serve seam (chunked continuation).
//!
//! The **first half** of the one event-acquisition mechanism: at
//! interest-open time, map the `InterestShape` → `StoreQuery` variants, scan
//! the store newest-first, and feed results through the **same post-store
//! projection-dispatch path** relay-delivered events take
//! (`insert_timeline_id_sorted` + `events` read-cache +
//! `notify_event_observers`) — NOT through `store.insert`, whose `Duplicate`
//! arm deliberately skips timeline append and observer fan-out (ADR §1.2).
//!
//! For kind:1059 DM gift-wraps the path additionally fires
//! `notify_raw_event_observers` (the verbatim-signed-event tap) so the
//! `DmInboxProjection` decrypt seam receives the full `sig`-bearing JSON —
//! one decrypt code path shared with live relay delivery (ADR R2.4(f)).
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
//! E1 conservatively refused to floor tag-/address-/event-id-filtered shapes
//! because serve did not cover them. E2 adds Ptag (kind:1059 DM inbox) and
//! E3 adds Etag (threads), KindDtag (addressable), and Ptag-mentions. Now
//! that every floored shape is covered by serve, the watermark is restored for
//! those shapes. The structural guard
//! `cache_serve_budget_tests::e3_structural_floored_implies_served` asserts
//! the invariant as a seam-identity check (not a per-shape checklist).

pub(super) mod continuation;
pub(super) mod queries;

pub(in crate::kernel) use queries::{
    completion_key_for_interest, cursor_less_query_key, query_since_mut, query_until_mut,
    shape_needs_ingest_parser_dispatch, shape_to_store_queries, watermark_from_queries,
};

use std::collections::HashSet;

use super::Kernel;
use crate::planner::InterestShape;
use crate::store::StoreQuery;

/// One queued (possibly partially-completed) store-cache serve. Owned by
/// `Kernel::pending_cache_serves`; queries are mutated in place to carry the
/// resume cursor (`until` lowered to the last visited `created_at`).
pub(super) struct PendingCacheServe {
    /// One-shot completion key — inserted into `served_interest_shapes` when
    /// this serve finishes (all queries exhausted or depth satisfied).
    pub(super) completion_key: u64,
    /// `StoreQuery` list derived from the interest shape at enqueue time.
    pub(super) queries: Vec<StoreQuery>,
    /// Index of the query currently being drained.
    pub(super) query_idx: usize,
    /// Events still to serve for this interest (starts at the consumer's
    /// visible window — see [`Kernel::serve_depth_for_shape`]).
    pub(super) remaining_depth: usize,
    /// Whether this serve feeds the follow-feed timeline (every shape author
    /// was in `timeline_authors` at enqueue time). Enables the
    /// aggregate-window `since` floor. Stale-flag safe: the flag only gates
    /// an optimization; the per-event `timeline_authors` check at feed time
    /// is the correctness gate.
    pub(super) timeline_bound: bool,
    /// Whether events collected during this serve should be dispatched through
    /// the `IngestParser` dispatcher in addition to `notify_event_observers`.
    /// Set at enqueue time by querying `EventIngestDispatcher::is_interested`
    /// for any kind in the shape — true when at least one registered parser
    /// would fire for those kinds. Covers DM gift-wraps (kind:1059), follow-feed
    /// notes (kind:1), and any other kind covered by a registered parser
    /// (including all-kinds range parsers like chirp-tui's `RawCacheIngestParser`).
    ///
    /// Note: this flag does NOT gate `notify_raw_event_observers` — the verbatim
    /// signed-event tap fires only on live relay ingest, never on cache-serve.
    pub(super) needs_ingest_parser_dispatch: bool,
}

impl Kernel {
    /// ADR-0045 single choke-point — queue a store-cache serve for an interest
    /// that was just installed (any install path), then drain ONE aggregate-budget
    /// chunk synchronously so the first snapshot after install carries store data
    /// (D1). Further work stays queued and continues on the actor tick (§5
    /// chunked continuation). Idempotent: completion keys already served or
    /// already queued are no-ops.
    ///
    /// The single key-derivation + enqueue + drain recipe. Every interest-install
    /// path funnels here so the completion-key derivation lives in exactly one
    /// place — no hand-copied recipe can drift from it (the lesson F3 of the
    /// PR #1237 review enforces).
    ///
    /// Reached through the two install front doors that own the recipe end to
    /// end ([`Kernel::push_interest_and_serve`],
    /// [`Kernel::ensure_interest_and_serve`]). Batch callers that enqueue many
    /// interests under ONE synchronous drain (the follow-feed sync) use
    /// [`Kernel::enqueue_interest_cache_serve_deferred`] + a single trailing
    /// [`Kernel::run_cache_serve_step`] instead, so a 300–500-follow cold start
    /// drains once, not per author.
    ///
    /// `pub(crate)` so `crate::actor::dispatch` can reach it without
    /// crossing the `pub(in crate::kernel)` boundary.
    pub(crate) fn enqueue_interest_cache_serve(
        &mut self,
        key: &crate::subs::SubKey,
        shape: &InterestShape,
    ) {
        self.enqueue_interest_cache_serve_deferred(key, shape);
        self.run_cache_serve_step();
    }

    /// Enqueue-only half of [`Kernel::enqueue_interest_cache_serve`] — derive
    /// the completion key and queue the serve WITHOUT draining a budget chunk.
    ///
    /// For batch installers (the follow-feed sync registers one interest per
    /// followed pubkey) that want N enqueues under ONE trailing
    /// [`Kernel::run_cache_serve_step`]. The completion-key derivation is shared
    /// with the single-install path, so the two cannot drift.
    pub(in crate::kernel) fn enqueue_interest_cache_serve_deferred(
        &mut self,
        key: &crate::subs::SubKey,
        shape: &InterestShape,
    ) {
        let completion_key = completion_key_for_interest(key, shape);
        self.enqueue_cache_serve(shape, completion_key);
    }

    /// Legacy-surface install recipe (the `ActorCommand::PushInterest` arm and
    /// the follow-feed sync's per-author path resolve to this shape of work).
    ///
    /// `push` uses `set_sub` (always-upsert): the shape may be newly installed
    /// or may have changed, so we always (a) enqueue the recompile trigger and
    /// (b) serve. Completion-key idempotency inside
    /// [`Kernel::enqueue_interest_cache_serve`] makes an unchanged shape a no-op.
    ///
    /// Centralised here (not inlined in the dispatch arm) so dispatch.rs stays
    /// under its file-size cap and the install recipe lives in one place.
    pub(crate) fn push_interest_and_serve(&mut self, interest: crate::planner::LogicalInterest) {
        let serve_key = crate::subs::InterestRegistry::legacy_key(&interest.id);
        let serve_shape = interest.shape.clone();
        self.lifecycle.registry_mut().push(interest);
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
                reason: crate::subs::InvalidateReason::External("push-interest".to_string()),
            });
        self.enqueue_interest_cache_serve(&serve_key, &serve_shape);
    }

    /// `ensure_sub` install recipe — register-if-absent, then enqueue a recompile
    /// trigger AND a store-cache serve ONLY when the interest was newly installed.
    ///
    /// The single front door for every register-if-absent install path:
    /// - the `ActorCommand::EnsureInterest` dispatch arm,
    /// - [`Kernel::open_interest_sub`] (the M2 `OpenInterest` seam),
    /// - the `nostr:` URI resolver (`kernel_action::open_uri`).
    ///
    /// Routing all three through here closes the F2 bypass the PR #1237 review
    /// found (open_uri installed an interest without serving store-resident
    /// events) and keeps the "trigger + serve only on newly-installed" invariant
    /// in one place so the call sites cannot drift.
    ///
    /// `reason` labels the recompile trigger for diagnostics. Returns `true`
    /// iff the interest was newly installed.
    pub(crate) fn ensure_interest_and_serve(
        &mut self,
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
        reason: &'static str,
    ) -> bool {
        let serve_key = identity.key;
        let serve_shape = interest.shape.clone();
        let newly_installed = self.lifecycle.registry_mut().ensure_sub(identity, interest);
        if newly_installed {
            self.lifecycle
                .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
                    reason: crate::subs::InvalidateReason::External(reason.to_string()),
                });
            self.enqueue_interest_cache_serve(&serve_key, &serve_shape);
        }
        newly_installed
    }

    /// Serve depth for one interest: 1× the consumer's visible window.
    ///
    /// `shape.limit` is the relay-wire backfill cap (the follow feed carries
    /// `Some(1000)`); the kernel's `visible_limit` is the consumer's render
    /// window. The serve depth is the smaller of the two — serving past the
    /// visible window is wasted actor work (ADR §4, owner decision
    /// 2026-06-12: depth = 1× visible window).
    fn serve_depth_for_shape(&self, shape: &InterestShape) -> usize {
        debug_assert!(self.visible_limit >= 1, "visible_limit must be ≥ 1");
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
    /// already served or already queued is a no-op. Shapes not covered by any
    /// engineering increment are marked served immediately (no retry, no queue
    /// entry).
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
            // Shape not covered — mark served so we don't re-derive.
            self.served_interest_shapes.insert(completion_key);
            return;
        }

        let timeline_bound = !shape.authors.is_empty()
            && shape
                .authors
                .iter()
                .all(|a| self.timeline_authors.contains(a));

        // Query the live dispatcher registrations to decide whether cache-served
        // events for this shape need `IngestParser` dispatch. Reads the lock
        // once at enqueue time; the flag is stable for the lifetime of the serve
        // (parsers are not removed mid-serve in normal operation).
        // D6 — a poisoned lock yields `None` → `false` (no dispatch), which is
        // the safe graceful-degrade: events reach `notify_event_observers` as
        // always; only the IngestParser fan-out is suppressed.
        let needs_ingest_parser_dispatch = self
            .ingest_dispatcher_slot()
            .read()
            .ok()
            .as_deref()
            .map(|d| shape_needs_ingest_parser_dispatch(shape, Some(d)))
            .unwrap_or(false);

        self.pending_cache_serves.push_back(PendingCacheServe {
            completion_key,
            queries,
            query_idx: 0,
            remaining_depth: self.serve_depth_for_shape(shape),
            timeline_bound,
            needs_ingest_parser_dispatch,
        });
    }

    /// Whether any cache-serve work is queued. The actor loop gates its
    /// per-tick [`Kernel::run_cache_serve_step`] call on this — an empty
    /// queue costs one bool check per wake (D8: no false-wakeup work).
    #[must_use]
    pub(crate) fn has_pending_cache_serves(&self) -> bool {
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
    pub(crate) fn run_cache_serve_step(&mut self) -> usize {
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

    /// Clear the served-interest completion set AND the pending serve queue.
    ///
    /// Must be called on account-switch / kernel reset so the next identity's
    /// interests get a fresh serve and the prior identity's queued serves do
    /// not keep draining.
    pub(in crate::kernel) fn clear_served_interest_shapes(&mut self) {
        self.served_interest_shapes.clear();
        self.pending_cache_serves.clear();
        // K3 Stage B3 / #1380: the truncation state is session-scoped (it tracks
        // which cursor-less serves stranded a tail this session). Account-switch
        // resets the serve state, so clear BOTH the completion-key set and its
        // query-key read view.
        if let Ok(mut set) = self.etag_ptag_truncated_serves.lock() {
            set.clear();
        }
        if let Ok(mut set) = self.etag_ptag_truncated_query_keys.lock() {
            set.clear();
        }
    }

    /// Rebuild the query-key read view ([`Kernel::etag_ptag_truncated_query_keys`])
    /// from the completion-key truncation set + the live interest registry
    /// (#1380 Bug 1).
    ///
    /// The completion-key set (`etag_ptag_truncated_serves`) is the SubKey-aware
    /// write surface: `serve_chunk` inserts/removes by `pending.completion_key`,
    /// so one interest's natural exhaustion never clears another's mark. But the
    /// floor decision is made from a SHAPE (the shape-only `watermark_fn` closure
    /// and the per-shape `shape_floor` probe cannot see `SubKey`), and two
    /// interests that share an Etag/Ptag shape collapse to ONE wire REQ with ONE
    /// `since` floor.
    ///
    /// So the floor for a cursor-less shape must be refused iff **at least one**
    /// active interest whose shape maps to that query is currently truncated.
    /// This method computes exactly that set: for every active interest, derive
    /// its `completion_key`; if that key is in the truncation set, add the
    /// `cursor_less_query_key` of the interest's cursor-less query to the read
    /// view. Called after every insert/remove on the completion-key set so the
    /// closure-visible view stays in lockstep.
    ///
    /// D6 graceful-degrade: a poisoned lock on either set leaves the view
    /// unchanged (the prior view is the safe-conservative default — at worst it
    /// refuses a floor that is no longer truncated, which only re-requests
    /// already-held events; it never drops events).
    pub(in crate::kernel) fn recompute_truncated_query_keys(&self) {
        let Ok(completion_keys) = self.etag_ptag_truncated_serves.lock() else {
            return;
        };
        let mut query_keys: HashSet<u64> = HashSet::new();
        if !completion_keys.is_empty() {
            for (sub_key, interest) in self.lifecycle.registry().iter_active_with_keys() {
                let completion_key = completion_key_for_interest(&sub_key, &interest.shape);
                if !completion_keys.contains(&completion_key) {
                    continue;
                }
                for query in shape_to_store_queries(&interest.shape) {
                    if let Some(qk) = cursor_less_query_key(&query) {
                        query_keys.insert(qk);
                    }
                }
            }
        }
        drop(completion_keys);
        if let Ok(mut view) = self.etag_ptag_truncated_query_keys.lock() {
            *view = query_keys;
        }
    }
}
