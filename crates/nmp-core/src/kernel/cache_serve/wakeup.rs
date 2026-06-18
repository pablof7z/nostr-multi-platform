//! Event-driven cache-serve wakeup (#1520).
//!
//! When a live event is accepted by the ingest chokepoint, already-served
//! interests may need a re-arm: the interest was served from the cold store
//! but a new matching event just landed and the consumer's projection needs
//! to see it. Without re-arming, a served projection would only update via
//! the normal `project_accepted_event` fan-out — which is correct for
//! the live event itself, but does NOT re-run the interest's cache-serve
//! pass so newly-arrived events that entered the store during the session
//! are included in subsequent snapshot projections ordered by the cache.
//!
//! ## Algorithm
//!
//! - `note_store_insert`: called from the live-ingest canonical path (after
//!   `project_accepted_event`). Iterates all active interests via the registry
//!   and for each one whose shape matches the event AND whose completion_key is
//!   in `served_interest_shapes` (i.e. already fully served), appends the
//!   completion_key to `cache_serve_wakeups`. Does NOT insert keys for
//!   interests that are still pending — they will finish naturally.
//!
//! - `drain_cache_serve_wakeups`: called as the first action of
//!   `run_cache_serve_step`. For each wakeup key it removes the key from
//!   `served_interest_shapes` (so `enqueue_cache_serve` will not skip it) and
//!   re-enqueues the matching interest for a fresh serve.
//!
//! ## D8 compliance
//!
//! No polling, no timers, no unbounded channels. The wakeup set is a
//! `BTreeSet<u64>` (bounded by the number of distinct interests, the same
//! bound as `served_interest_shapes`). The coalesce property: many rapid
//! store-inserts for the same interest produce exactly one BTreeSet entry.

use super::{completion_key_for_interest, super::Kernel};

impl Kernel {
    /// Record that a live store insert matched active interests.
    ///
    /// Called from the canonical accepted-event path in `ingest/accepted.rs`
    /// (after `project_accepted_event`) for `Inserted | Replaced | Ephemeral`
    /// outcomes. Iterates all registered `(SubKey, LogicalInterest)` pairs,
    /// checks if the event matches the interest's shape, and if so checks
    /// whether the interest is already fully served (completion key in
    /// `served_interest_shapes`). Only fully-served interests are armed for
    /// re-serve — pending interests will reach the event naturally when their
    /// serve finishes.
    pub(in crate::kernel) fn note_store_insert(
        &mut self,
        event_id: &str,
        author: &str,
        kind: u32,
        created_at: u64,
        tags: &[Vec<String>],
    ) {
        // Snapshot the active interests so we can borrow self.served_interest_shapes
        // without a split-borrow conflict.
        let active = self.lifecycle.registry().iter_active_with_keys();
        for (sub_key, interest) in &active {
            if interest
                .shape
                .matches_event_with_id(event_id, author, kind, created_at, tags)
            {
                let key = completion_key_for_interest(sub_key, &interest.shape);
                if self.served_interest_shapes.contains(&key) {
                    self.cache_serve_wakeups.insert(key);
                }
            }
        }
    }

    /// Drain the coalesced wakeup set, re-arming each affected interest for a
    /// fresh cache-serve pass.
    ///
    /// Called as the first action of `run_cache_serve_step`. For each wakeup
    /// key:
    /// 1. Remove from `served_interest_shapes` so `enqueue_cache_serve` won't
    ///    skip the interest.
    /// 2. Find the matching `(SubKey, LogicalInterest)` in the active registry.
    /// 3. Re-enqueue via `enqueue_interest_cache_serve_deferred`.
    ///
    /// After draining, the wakeup set is empty until the next `note_store_insert`
    /// call fires new matches.
    pub(in crate::kernel) fn drain_cache_serve_wakeups(&mut self) {
        if self.cache_serve_wakeups.is_empty() {
            return;
        }
        // Collect wakeup keys to process (drain the set atomically).
        let wakeup_keys: Vec<u64> = self.cache_serve_wakeups.iter().copied().collect();
        self.cache_serve_wakeups.clear();

        // Snapshot active interests once to avoid repeated registry reads.
        let active = self.lifecycle.registry().iter_active_with_keys();

        for wakeup_key in wakeup_keys {
            // Remove from the completion set so enqueue_cache_serve won't skip.
            self.served_interest_shapes.remove(&wakeup_key);

            // Find the interest matching this wakeup key in the current registry.
            // If it has been dropped (last owner left) since the wakeup was armed,
            // there is no entry to re-enqueue — silently skip.
            for (sub_key, interest) in &active {
                let candidate_key = completion_key_for_interest(sub_key, &interest.shape);
                if candidate_key == wakeup_key {
                    self.enqueue_interest_cache_serve_deferred(sub_key, &interest.shape);
                    break;
                }
            }
        }
    }

    /// Whether there are coalesced wakeup keys waiting to arm a cache-serve
    /// pass. The actor loop checks this alongside `has_pending_cache_serves`
    /// to decide whether to call `run_cache_serve_step`.
    #[must_use]
    pub(crate) fn has_cache_serve_wakeups(&self) -> bool {
        !self.cache_serve_wakeups.is_empty()
    }
}
