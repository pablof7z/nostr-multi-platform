//! ADR-0062 — observer-scoped read-model catch-up.
//!
//! When a late-joining per-open feed (Chirp author/thread profile) registers a
//! `KernelEventObserver` AFTER events matching its interest have already been
//! accepted and cached in the in-memory read-cache (`Kernel::events`), it
//! misses those events because the global `notify_observers` fan-out is
//! one-shot and per-observer activation replay is not part of the live ingest
//! path.
//!
//! This module provides the `open_interest_with_observer_replay` kernel method
//! that wires together:
//!
//! 1. `register_interest` — the normal interest front-door (`EnsureAbsent`),
//!    which triggers the relay-subscribe path. Unchanged and DO NOT TOUCH.
//! 2. `replay_read_cache_to_observer` — scan the in-memory `events` read-cache
//!    for matching events and deliver them to the specific muted observer via
//!    `notify_event_observer_by_id`. This deliberately targets ONLY the one
//!    observer, not the global fan-out, so already-active observers do NOT
//!    receive duplicate deliveries.
//! 3. `activate_observer` — promote the muted observer to active so subsequent
//!    global `notify_observers` calls reach it.
//!
//! ## Dedup invariant
//!
//! The replay step reads from `self.events` (the in-memory read-cache) which is
//! populated by `project_accepted_event` ONLY for events that passed through the
//! live ingest chokepoint (ADR-0057). Events served from the store by the
//! cache-serve path (continuation.rs) are deduplicated against this same cache
//! (`events_cache.contains_key` — line 99, DO NOT TOUCH). So replaying from
//! `self.events` means the observer sees exactly the events that already fired
//! the global fan-out — no store query, no double-count risk.
//!
//! ## D9 clock clamp
//!
//! `replay_read_cache_to_observer` clamps `created_at` to `now_secs()` for
//! future-dated events, mirroring `ingest/projection.rs:87`. The authoritative
//! store retains the original timestamp; the observer receives only the
//! display-safe clamped value.
//!
//! ## Ordering
//!
//! Events are selected newest `limit` (sorted by `(created_at, id)`), then
//! delivered oldest-first (the tail of the sorted slice), mirroring
//! `continuation.rs:124` — so the feed observer applies events in the same
//! chronological order as the live and cache-serve ingest paths.

use super::Kernel;
use crate::actor::KernelEventObserverId;
use crate::kernel::cache_serve::{InterestRegistration, InterestWrite};
use crate::planner::{InterestShape, LogicalInterest};
use crate::subs::SubIdentity;
use crate::substrate::KernelEvent;

/// Replay request carrying the registration parameters for targeted
/// read-cache catch-up (ADR-0062 §6).
pub(crate) struct ObserverReplayRequest {
    /// The muted observer id to deliver replayed events to.
    pub observer_id: KernelEventObserverId,
    /// Filter shapes used to match events in the read-cache.
    /// A single `OpenObservedInterest` command may carry multiple shapes
    /// (e.g. thread feed: `#e` replies shape + root-by-id shape) — all
    /// matching events across all shapes are delivered (union).
    pub shapes: Vec<InterestShape>,
    /// Maximum number of events to replay (newest-first selection,
    /// oldest-first delivery). Use the feed's visible window limit.
    pub limit: usize,
}

impl Kernel {
    /// ADR-0062 — open an interest AND immediately replay matching in-memory
    /// cached events to the nominated muted observer, then activate it.
    ///
    /// Calls `register_interest` with `EnsureAbsent` (unchanged relay-subscribe
    /// path), then UNCONDITIONALLY replays `self.events` to the observer
    /// (regardless of whether `changed` is true — the slot may already exist
    /// from a previous multi-owner open, but the new observer still needs its
    /// catch-up). Then promotes the observer from muted to active.
    ///
    /// Returns `true` iff the interest was newly installed.
    pub(crate) fn open_interest_with_observer_replay(
        &mut self,
        identity: SubIdentity,
        interest: LogicalInterest,
        replay: ObserverReplayRequest,
        reason: &'static str,
    ) -> bool {
        // Step 1: normal interest registration (UNCHANGED — do not gate replay
        // on the `changed` outcome; a multi-owner slot returns changed:false
        // for a second subscriber but the new observer still needs its replay).
        let outcomes = self.register_interest(
            &[InterestRegistration {
                identity,
                interest,
                policy: InterestWrite::EnsureAbsent,
            }],
            reason,
        );

        // Step 2: targeted read-cache replay. Always runs — the observer is
        // muted and has received nothing yet regardless of the slot state.
        let replayed = self.replay_read_cache_to_observer(&replay);
        let _ = replayed; // count is informational; callers don't need it

        // Step 3: promote muted → active so future global fan-out includes it.
        if let Some(slot) = &self.event_observers {
            crate::actor::activate_observer(slot, replay.observer_id);
        }

        outcomes[0].newly_installed
    }

    /// Scan `self.events` (the in-memory read-cache) for entries matching any
    /// of `replay.shapes`, select the newest `replay.limit` events, and
    /// deliver them oldest-first to `replay.observer_id` via
    /// `notify_event_observer_by_id`.
    ///
    /// # Invariants
    ///
    /// - MUST NOT call `feed_served_event` / `project_accepted_event` /
    ///   `note_store_mutation` / `cache_event_for_matching_open_interest`.
    /// - MUST NOT mutate `self.events`, metrics, served-interest state,
    ///   pending serves, wakeups, or `changed_since_emit`.
    /// - `created_at` is D9-clamped to `now_secs()` (future-dated defence,
    ///   same as `ingest/projection.rs:87`).
    ///
    /// Returns the number of events delivered.
    fn replay_read_cache_to_observer(&self, replay: &ObserverReplayRequest) -> usize {
        if replay.shapes.is_empty() || replay.limit == 0 {
            return 0;
        }

        let now = self.now_secs();

        // Collect all matching entries from the read-cache (owned copies so
        // we don't hold a borrow across the mutable `notify_event_observer_by_id`).
        struct CachedEntry {
            created_at: u64,
            id: String,
            author: String,
            kind: u32,
            tags: Vec<Vec<String>>,
            content: String,
        }

        let mut matched: Vec<CachedEntry> = Vec::new();
        for (id, stored) in &self.events {
            let matches = replay.shapes.iter().any(|shape| {
                shape.matches_event_with_id(
                    id,
                    &stored.author,
                    stored.kind,
                    stored.created_at,
                    &stored.tags,
                )
            });
            if matches {
                matched.push(CachedEntry {
                    created_at: stored.created_at,
                    id: id.clone(),
                    author: stored.author.clone(),
                    kind: stored.kind,
                    tags: stored.tags.clone(),
                    content: stored.content.clone(),
                });
            }
        }

        if matched.is_empty() {
            return 0;
        }

        // Select newest `limit` events: sort ascending by (created_at, id),
        // then keep the tail (newest-first conceptually, but we deliver
        // oldest-first so we use the tail of the ascending sort).
        matched.sort_unstable_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
        let start = matched.len().saturating_sub(replay.limit);

        // Deliver oldest-first (the window is already in ascending
        // created_at order — mirrors continuation.rs:124 where collected
        // is reversed to feed oldest-first).
        let mut delivered = 0;
        for entry in &matched[start..] {
            let event = KernelEvent {
                id: entry.id.clone(),
                author: entry.author.clone(),
                kind: entry.kind,
                // D9: clamp future-dated events to now (mirrors
                // ingest/projection.rs:87).
                created_at: entry.created_at.min(now),
                tags: entry.tags.clone(),
                content: entry.content.clone(),
                // relay_provenance filled by notify_event_observer_by_id via
                // the store lookup (same path as notify_event_observers).
                relay_provenance: Vec::new(),
            };
            if self.notify_event_observer_by_id(replay.observer_id, &event) {
                delivered += 1;
            }
        }
        delivered
    }
}
