//! ADR-0070 Rung 3 — the host-declared incremental-apply capability seam.
//!
//! Extracted from `snapshot_registry.rs` (the `impl SnapshotRegistry` methods
//! that read/write the two `incremental_apply_*` fields) so that file stays
//! under the 500-LOC hard ceiling (AGENTS.md file-size rule) — the same
//! submodule pattern `entry.rs` / `kernel_access.rs` already use for this file.
//!
//! The two fields themselves (`incremental_apply_enabled` /
//! `incremental_apply_baseline_pending`) remain on the `SnapshotRegistry`
//! struct definition in the parent module; only the inherent methods that
//! manipulate them live here.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use super::SnapshotRegistry;

impl SnapshotRegistry {
    /// ADR-0070 R6-S1 — a clone of the single-source-of-truth incremental-apply
    /// flag, for a Tier-1 producer closure to read lock-free.
    ///
    /// The closure runs inside `run_typed()` while this registry's mutex is held,
    /// so it MUST NOT re-lock the registry — it reads this captured
    /// `Arc<AtomicBool>` directly. The atomic is the SAME one
    /// [`Self::is_incremental_apply_enabled`] reads and
    /// [`Self::declare_incremental_apply`] writes; capturing a clone introduces
    /// no second flag.
    #[must_use]
    pub fn incremental_apply_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.incremental_apply_enabled)
    }

    /// ADR-0070 R6-S1 — clones of the frame-identity handles
    /// `(session_id, snapshot_epoch)` the kernel publishes each tick.
    ///
    /// A Tier-1 producer closure that omits unchanged frames captures these and
    /// forces a rebaseline whenever EITHER value changes — the same signal the
    /// host cache resets on. Read lock-free (Acquire) inside the closure; never
    /// re-locks the registry. See `SnapshotRegistry::frame_session_id`.
    #[must_use]
    pub fn frame_identity_handles(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>) {
        (
            Arc::clone(&self.frame_session_id),
            Arc::clone(&self.frame_snapshot_epoch),
        )
    }

    /// ADR-0070 R6-S1 — publish the current frame identity into the shared
    /// handles. Called by the kernel at the top of `make_update` (before the
    /// typed projections run), so the producer closure reads THIS tick's values.
    ///
    /// Lock-free `Release` stores; the closure reads with `Acquire`. Writing on
    /// every tick is idempotent and cheap (two atomic stores) — the values only
    /// actually change on Reset (`session_id`) or epoch bump (`snapshot_epoch`).
    pub fn publish_frame_identity(&self, session_id: u64, snapshot_epoch: u64) {
        self.frame_session_id.store(session_id, Ordering::Release);
        self.frame_snapshot_epoch
            .store(snapshot_epoch, Ordering::Release);
    }

    /// ADR-0070 D7 (#1671 Lane H) — a clone of the per-tick rev handle.
    ///
    /// A [`FeedWindowSource`](nmp_feed::FeedWindowSource) captures this and reads
    /// it lock-free to key its per-tick window memo, so the author provider and
    /// the typed producer (two reads in one tick) share one materialization.
    #[must_use]
    pub fn frame_tick_rev_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.frame_tick_rev)
    }

    /// ADR-0070 D7 (#1671 Lane H) — bump the per-tick rev. Called by the kernel at
    /// the TOP of `make_update` (alongside `publish_frame_identity`), BEFORE any
    /// feed-author provider or typed producer runs, so every closure this tick
    /// reads the SAME rev. A monotone `Release` add (the readers use `Acquire`).
    pub fn bump_frame_tick_rev(&self) {
        self.frame_tick_rev.fetch_add(1, Ordering::Release);
    }
    /// ADR-0070 Rung 3 — declare that this host's runtime owns the NMP
    /// cache-merge layer (D3-3) and can therefore receive frames with
    /// `Unchanged` projections omitted.
    ///
    /// Single-writer, set before `nmp_app_start`. After this call the kernel
    /// MUST emit a full baseline on the next `make_update` tick (all live
    /// Tier-2 projections as `Changed`) — enforced by setting a
    /// `baseline_pending` latch that `make_update` drains via
    /// `take_incremental_apply_baseline_pending`, triggering
    /// `ProjectionRevTracker::reset_last_emitted` (D3-5).
    ///
    /// Idempotent: calling more than once before start is a no-op.
    ///
    /// R6-S1: `incremental_apply_enabled` is an `Arc<AtomicBool>` (the single
    /// source of truth shared with any Tier-1 producer closure that gates its
    /// own omit). A `Release` store here is observed by the closure's `Acquire`
    /// load. Single-writer (set-before-start), so the load-then-store is not a
    /// contended RMW race.
    pub fn declare_incremental_apply(&mut self) {
        if !self.incremental_apply_enabled.load(Ordering::Acquire) {
            self.incremental_apply_enabled
                .store(true, Ordering::Release);
            // D3-5: signal that the kernel must reset its last-emitted baseline
            // so the next frame is a full baseline. The latch is consumed once
            // by `take_incremental_apply_baseline_pending` in `make_update`.
            self.incremental_apply_baseline_pending = true;
        }
    }

    /// Read whether the host has declared incremental-apply capability.
    ///
    /// The kernel reads this once per tick (inside `make_update`) to decide
    /// whether to pass `enabled = true` to `rung3_omit::omit_unchanged`. Reads
    /// the same `Arc<AtomicBool>` a producer closure captures via
    /// [`SnapshotRegistry::incremental_apply_handle`] (single source of truth).
    #[must_use]
    pub fn is_incremental_apply_enabled(&self) -> bool {
        self.incremental_apply_enabled.load(Ordering::Acquire)
    }

    /// ADR-0070 Rung 3 (D3-5) — take the "baseline pending" latch.
    ///
    /// Returns `true` exactly once after `declare_incremental_apply` sets the
    /// latch. The caller (`make_update`) must then call
    /// `ProjectionRevTracker::reset_last_emitted` so the next frame is a full
    /// baseline for the newly-attached incremental host.
    pub fn take_incremental_apply_baseline_pending(&mut self) -> bool {
        if self.incremental_apply_baseline_pending {
            self.incremental_apply_baseline_pending = false;
            true
        } else {
            false
        }
    }
}
