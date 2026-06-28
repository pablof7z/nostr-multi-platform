//! Kernel-side accessors over the shared [`SnapshotProjectionSlot`].
//!
//! Extracted from `snapshot_registry.rs` to keep that file within its LOC
//! ceiling. These are the methods `make_update` (and the `Reset` dispatch arm)
//! call to read the host-extensible registry through the `Arc<Mutex<…>>` slot the
//! actor binds onto the kernel: the typed projection runs and — ADR-0053 — the
//! host-declared consumed-projection set.

use super::super::Kernel;
use super::{DeclaredProjections, SnapshotProjectionSlot};
use crate::update_envelope::TypedProjectionData;

impl Kernel {
    /// Install the actor's shared snapshot-projection slot.
    ///
    /// The `Arc<Mutex<…>>` is shared with the FFI surface
    /// (`ffi/snapshot.rs`) and any per-app crate that registered a
    /// projection; the same registrations are therefore visible to both the
    /// actor thread and external Rust callers. Idempotent — re-binding
    /// replaces the prior handle. The actor calls this once immediately after
    /// constructing a kernel.
    pub(crate) fn set_snapshot_projection_handle(&mut self, handle: SnapshotProjectionSlot) {
        self.snapshot_projections = Some(handle);
    }

    /// Extract the snapshot-projection handle before a `Reset` replaces the
    /// kernel. The slot's `Arc<Mutex<…>>` is shared with the FFI surface and
    /// per-app crates, so it MUST survive `Reset` — otherwise every host
    /// projection (and the declared set) would silently stop appearing (the same
    /// survival contract as the event observer slot).
    pub(crate) fn take_snapshot_projection_handle_for_reset(
        &mut self,
    ) -> Option<SnapshotProjectionSlot> {
        self.snapshot_projections.take()
    }

    /// Run every registered **typed** snapshot projection and return the vector
    /// carried in the snapshot frame's `typed_projections` sidecar (ADR-0037).
    ///
    /// Empty when no slot is bound, the mutex is poisoned, or nothing is
    /// registered — D6: a projection failure is data, never a panic at the
    /// boundary. Called from `make_update`.
    pub(in crate::kernel) fn run_typed_projections(&self) -> Vec<TypedProjectionData> {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|mut registry| registry.run_typed_at(self.now_secs()))
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// ADR-0063 D7 (#1671 Lane H) — collect every registered feed-author
    /// provider's CURRENT visible-author set for this tick, as
    /// `(feed_key, keys)`.
    ///
    /// Reads through the shared slot; empty when no slot is bound, the mutex is
    /// poisoned, or nothing is registered (D6). Called from `make_update`; the
    /// kernel then reconciles each set against the prior tick via
    /// [`Kernel::reconcile_feed_author_refs`].
    pub(in crate::kernel) fn collect_feed_author_sets(&self) -> Vec<(String, Vec<String>)> {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.run_feed_author_providers())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// ADR-0053 — snapshot the host-declared consumed-projection set for this
    /// tick.
    ///
    /// Cloned ONCE at the top of `snapshot_projections_with_publish_cluster` so
    /// the per-key `permits()` checks don't re-lock the registry mutex for every
    /// Tier-2 built-in. When no slot is bound or the mutex is poisoned the result
    /// is `DeclaredProjections::default()` — which `permits()` everything in
    /// production (`Undeclared`) (D6: a gate-read failure degrades to "emit all",
    /// never to "drop all built-ins", and never a panic at the boundary). The
    /// loud forgotten-declaration check is at `nmp_app_start`, against the host's
    /// *actual* declared state — never against this degraded fallback, so a
    /// poisoned-mutex/no-slot read never false-fires the assert.
    pub(in crate::kernel) fn declared_projections_snapshot(&self) -> DeclaredProjections {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.declared_projections().clone())
                .unwrap_or_default(),
            None => DeclaredProjections::default(),
        }
    }

    /// ADR-0055 R6-S1 — publish this tick's frame identity
    /// `(session_id, snapshot_epoch)` into the shared registry handles so a
    /// Tier-1 producer closure (the feed change-signal) reads the SAME signal
    /// the host's `ProjectionCache` resets on.
    ///
    /// MUST be called at the TOP of `make_update`, before ANY host projection
    /// closure runs (typed projections run in `run_typed_projections`) — so every
    /// closure this tick sees the current values. `session_id` =
    /// `TimingMilestones::started_unix_ms` (changes on Reset-rebuild);
    /// `snapshot_epoch` = `ProjectionRevTracker::epoch` (account-switch /
    /// schema bump). A no-op when no slot is bound or the mutex is poisoned
    /// (D6: the producer then never rebaselines on identity — but with no slot
    /// bound there is no producer either, so this is vacuously safe).
    pub(in crate::kernel) fn publish_frame_identity(&self) {
        let session_id = self.timing.started_unix_ms.unwrap_or(0);
        let snapshot_epoch = self.projection_rev_tracker.epoch;
        if let Some(slot) = &self.snapshot_projections {
            if let Ok(registry) = slot.lock() {
                registry.publish_frame_identity(session_id, snapshot_epoch);
                // ADR-0063 D7 (#1671 Lane H) — advance the per-tick rev in the
                // SAME lock so every feed-author provider + typed producer this
                // tick reads one rev and materializes its window exactly once.
                registry.bump_frame_tick_rev();
            }
        }
    }

    /// ADR-0063 D7 (#1671 Lane H) — the per-tick rev for THIS `make_update`.
    ///
    /// Read AFTER [`Self::publish_frame_identity`] has bumped it, so it matches the
    /// rev every feed closure used this tick. Used to drain the emitted-author
    /// sink for the structural guardrail. Returns `0` when no slot is bound (no
    /// feeds, so nothing to check).
    pub(in crate::kernel) fn current_frame_tick_rev(&self) -> u64 {
        use std::sync::atomic::Ordering;
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.frame_tick_rev_handle().load(Ordering::Acquire))
                .unwrap_or(0),
            None => 0,
        }
    }

    /// ADR-0063 D7 (#1671 Lane H) — drain the emitted-author sink for the tick
    /// matching `tick_rev`: `(consumer_id, author_key)` pairs every feed's typed
    /// producer recorded as ACTUALLY EMITTED onto the wire this tick.
    ///
    /// Empty when no slot is bound, the mutex is poisoned, or no feed recorded
    /// this tick (D6). Read by [`Kernel::warn_emitted_unresolved_feed_authors`]
    /// AFTER the typed projections are emitted.
    pub(in crate::kernel) fn emitted_feed_authors(&self, tick_rev: u64) -> Vec<(String, String)> {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|registry| registry.emitted_feed_authors_for_tick(tick_rev))
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// ADR-0055 Rung 3 S1b (finding 6 / issue #1390) — read both the
    /// incremental-apply enabled flag and the baseline-pending latch in a
    /// **single** mutex acquisition.
    ///
    /// Returns `(enabled, baseline_pending)`:
    /// - `enabled`: whether the host has declared incremental-apply capability.
    /// - `baseline_pending`: whether a full-baseline reset is pending (consumed
    ///   exactly once — subsequent calls return `false` for `baseline_pending`
    ///   until `declare_incremental_apply` is called again).
    ///
    /// When no slot is bound or the mutex is poisoned, returns `(false, false)`
    /// (D6: degrades to full rows + no reset needed).
    pub(in crate::kernel) fn incremental_apply_state(&mut self) -> (bool, bool) {
        match &self.snapshot_projections {
            Some(slot) => slot
                .lock()
                .map(|mut registry| {
                    let enabled = registry.is_incremental_apply_enabled();
                    let baseline_pending = registry.take_incremental_apply_baseline_pending();
                    (enabled, baseline_pending)
                })
                .unwrap_or((false, false)),
            None => (false, false),
        }
    }
}
