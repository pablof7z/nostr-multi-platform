//! ADR-0055 Rung 3 — the host-declared incremental-apply capability seam.
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

use super::SnapshotRegistry;

impl SnapshotRegistry {
    /// ADR-0055 Rung 3 — declare that this host's runtime owns the NMP
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
    pub fn declare_incremental_apply(&mut self) {
        if !self.incremental_apply_enabled {
            self.incremental_apply_enabled = true;
            // D3-5: signal that the kernel must reset its last-emitted baseline
            // so the next frame is a full baseline. The latch is consumed once
            // by `take_incremental_apply_baseline_pending` in `make_update`.
            self.incremental_apply_baseline_pending = true;
        }
    }

    /// Read whether the host has declared incremental-apply capability.
    ///
    /// The kernel reads this once per tick (inside `make_update`) to decide
    /// whether to pass `enabled = true` to `rung3_omit::omit_unchanged`.
    #[must_use]
    pub fn is_incremental_apply_enabled(&self) -> bool {
        self.incremental_apply_enabled
    }

    /// ADR-0055 Rung 3 (D3-5) — take the "baseline pending" latch.
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
