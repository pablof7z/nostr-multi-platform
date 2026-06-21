//! ADR-0063 (#1671 integration glue, codex "Artifact 1") — the `refs.profile` /
//! `refs.event` row-delta producer slice of the Tier-2 built-in typed sidecar.
//!
//! Unlike every other Tier-2 cluster (which encodes a whole-map snapshot every
//! tick), these two keys carry a per-KEY row-delta batch (Lane A's NRRD carrier):
//! a steady-state incremental batch (only changed/cleared rows) or a full
//! baseline (every live row). The batch is built by [`crate::refs::RefRowDeltaTracker`]
//! over the kernel's own [`crate::refs::RefRowRevSource`] impl
//! (`kernel/ref_row_source.rs`), so the producer NEVER reimplements resolution.
//!
//! ## Why this runs in `make_update`, not `builtin_typed_projections`
//!
//! The producer needs `&mut self` — it advances the tracker's per-host
//! last-emitted-rev map and the kernel's baselined-identity latch each tick.
//! `builtin_typed_projections(&self)` is `&self`-only (a pure snapshot encode),
//! so the refs.* producer is hooked directly into `make_update` between the
//! baseline-latch reset and the manifest assembly (where live `&mut self` is
//! available). See `kernel/update.rs`.
//!
//! ## Baseline semantics (ADR-0063 invariant #3 / ADR-0055 D4)
//!
//! The tracker is BASELINED (`.reset()` → next build is a full re-emit) when:
//! - `baseline_pending` (a host just declared incremental-apply — first attach),
//! - the frame `session_id` changed (kernel `Reset`-rebuild — though a rebuild
//!   already drops the tracker, this is the belt for a shared-process reattach),
//! - the `snapshot_epoch` changed (account-switch / schema bump).
//!
//! This is the SAME `(session_id, epoch)` signal the host `RefRowCache`
//! rebaselines on, so producer and host stay in lockstep.

use crate::refs::encode_ref_row_delta_batch;
use crate::update_envelope::TypedProjectionData;

use super::super::ref_row_source::{REF_NS_EVENT, REF_NS_PROFILE};

/// The manifest / sidecar key for the keyed profile row-delta projection.
pub(crate) const REFS_PROFILE_KEY: &str = "refs.profile";
/// The manifest / sidecar key for the keyed event row-delta projection.
pub(crate) const REFS_EVENT_KEY: &str = "refs.event";
/// NRRD wire schema version (bump on any breaking change to `ref_rowdelta.fbs`).
const REFS_SCHEMA_VERSION: u32 = 1;
/// FlatBuffers file identifier every NRRD batch carries (`schema/ref_rowdelta.fbs`).
const REFS_FILE_IDENTIFIER: &str = "NRRD";

impl super::super::Kernel {
    /// Build the two `refs.*` row-delta typed sidecar entries for this tick.
    ///
    /// Runs from `make_update` with live `&mut self` (it mutates the tracker +
    /// the baselined-identity latch). Returns BOTH entries unconditionally (they
    /// are unconditional Tier-2 keys): the Rung-2 stamp marks each Changed /
    /// Unchanged from the manifest, and Rung-3 drops an Unchanged entry from the
    /// wire when the host advertises incremental-apply. Always returning both
    /// entries keeps the rung3_omit "unconditional Tier-2 key absent while
    /// manifest-Changed" invariant satisfied.
    ///
    /// `baseline_pending` is the one-shot latch already consumed by
    /// `incremental_apply_state()` this tick; pass it in so a fresh host attach
    /// forces a full re-baseline of the carrier too.
    pub(in crate::kernel) fn refs_row_delta_projections(
        &mut self,
        baseline_pending: bool,
    ) -> Vec<TypedProjectionData> {
        let session_id = self.timing.started_unix_ms.unwrap_or(0);
        let epoch = self.projection_rev_tracker.epoch;
        let identity = (session_id, epoch);
        let identity_changed = self.ref_row_last_identity != Some(identity);
        let baseline = baseline_pending || identity_changed;

        if baseline {
            // Full re-emit: drop the per-host last-emitted map so build_baseline
            // re-seeds every live row as Changed (ADR-0063 invariant #3).
            self.ref_row_delta_tracker.reset();
        }
        self.ref_row_last_identity = Some(identity);

        // The tracker borrows `&self` (the RefRowRevSource impl) while it is a
        // field of `self`; split the borrow by moving the tracker out, building
        // against the now-immutable `&self`, then putting it back. This is the
        // standard self-field-mutates-via-self-trait pattern (mirrors the
        // RefResolver enum-dispatch rationale in `kernel/refs.rs`).
        let mut tracker = std::mem::take(&mut self.ref_row_delta_tracker);
        let profile_batch = if baseline {
            tracker.build_baseline(REF_NS_PROFILE, self)
        } else {
            tracker.build_incremental(REF_NS_PROFILE, self)
        };
        let event_batch = if baseline {
            tracker.build_baseline(REF_NS_EVENT, self)
        } else {
            tracker.build_incremental(REF_NS_EVENT, self)
        };
        self.ref_row_delta_tracker = tracker;

        vec![
            ref_row_typed_projection(REFS_PROFILE_KEY, encode_ref_row_delta_batch(&profile_batch)),
            ref_row_typed_projection(REFS_EVENT_KEY, encode_ref_row_delta_batch(&event_batch)),
        ]
    }
}

/// Wrap an encoded NRRD batch in a `TypedProjectionData`. `projection_rev` /
/// `state` are stamped by `make_update` from the manifest after this returns.
fn ref_row_typed_projection(key: &str, payload: Vec<u8>) -> TypedProjectionData {
    TypedProjectionData {
        key: key.to_string(),
        schema_id: key.to_string(),
        schema_version: REFS_SCHEMA_VERSION,
        file_identifier: REFS_FILE_IDENTIFIER.to_string(),
        payload,
        ..Default::default()
    }
}
