//! ADR-0070 (#1671 integration glue, codex "Artifact 1") — the `refs.profile` /
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
//! ## Baseline semantics (ADR-0070 invariant #3 / ADR-0070 D4)
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
    ///
    /// `profile_permitted` / `event_permitted` are this tick's ADR-0070
    /// declared-set verdict for each `refs.*` key (`declared.permits(key)`).
    /// They gate the tracker per namespace so the tracker is NEVER advanced
    /// while a key is filtered off the wire:
    ///
    /// - UNPERMITTED → skip the build entirely (no tracker mutation, return an
    ///   empty batch). The retained `is_narrowing` filter in `make_update`
    ///   drops the entry anyway; not advancing the tracker keeps it at its
    ///   last-permitted state so live rows are NOT silently consumed.
    /// - PERMITTED after being UNPERMITTED last tick (a false→true ADR-0070
    ///   ADDITIVE declaration) → force a full BASELINE for that namespace
    ///   (`reset_namespace` + `build_baseline`) so a newly-declaring host
    ///   receives the complete live row set — exactly as other built-ins
    ///   re-baseline a newly-declared projection (see
    ///   `ProjectionRevTracker::reconcile_declared_permits`).
    /// - PERMITTED and was permitted (steady state) → ordinary baseline /
    ///   incremental per the `(session_id, epoch)` identity latch.
    pub(in crate::kernel) fn refs_row_delta_projections(
        &mut self,
        baseline_pending: bool,
        profile_permitted: bool,
        event_permitted: bool,
    ) -> Vec<TypedProjectionData> {
        let session_id = self.timing.started_unix_ms.unwrap_or(0);
        let epoch = self.projection_rev_tracker.epoch;
        let identity = (session_id, epoch);
        let identity_changed = self.ref_row_last_identity != Some(identity);
        let identity_baseline = baseline_pending || identity_changed;
        self.ref_row_last_identity = Some(identity);

        // A false→true permit transition (host ADDITIVELY declared this refs.*
        // key) forces a full per-namespace re-baseline so the newly-declaring
        // host gets every live row, not an empty incremental. Read the prior
        // per-key permit state, then record this tick's verdict for the next.
        let (prev_profile, prev_event) = self.ref_row_last_permits;
        let profile_newly_permitted = profile_permitted && !prev_profile;
        let event_newly_permitted = event_permitted && !prev_event;
        self.ref_row_last_permits = (profile_permitted, event_permitted);

        // The tracker borrows `&self` (the RefRowRevSource impl) while it is a
        // field of `self`; split the borrow by moving the tracker out, building
        // against the now-immutable `&self`, then putting it back. This is the
        // standard self-field-mutates-via-self-trait pattern (mirrors the
        // RefResolver enum-dispatch rationale in `kernel/refs.rs`).
        let mut tracker = std::mem::take(&mut self.ref_row_delta_tracker);
        let profile_batch = build_namespace_batch(
            &mut tracker,
            self,
            REF_NS_PROFILE,
            profile_permitted,
            identity_baseline || profile_newly_permitted,
        );
        let event_batch = build_namespace_batch(
            &mut tracker,
            self,
            REF_NS_EVENT,
            event_permitted,
            identity_baseline || event_newly_permitted,
        );
        self.ref_row_delta_tracker = tracker;

        vec![
            ref_row_typed_projection(REFS_PROFILE_KEY, encode_ref_row_delta_batch(&profile_batch)),
            ref_row_typed_projection(REFS_EVENT_KEY, encode_ref_row_delta_batch(&event_batch)),
        ]
    }
}

/// Build one namespace's row-delta batch, honouring the per-key ADR-0070 permit
/// gate. Returns an EMPTY (non-baseline, no-row) batch WITHOUT advancing the
/// tracker when the key is unpermitted this tick — the retained `is_narrowing`
/// filter drops it off the wire and the tracker must not record live rows as
/// already-emitted. When permitted, builds a full baseline (on `baseline`) or a
/// steady-state incremental, re-seeding the per-namespace last-emitted state on
/// a forced baseline first (so a newly-declared host gets the full live set).
fn build_namespace_batch(
    tracker: &mut crate::refs::RefRowDeltaTracker,
    source: &impl crate::refs::RefRowRevSource,
    namespace: &str,
    permitted: bool,
    baseline: bool,
) -> crate::refs::RefRowDeltaBatch {
    if !permitted {
        // Unpermitted: emit nothing, advance nothing. The is_narrowing filter
        // in make_update drops this entry; leaving the tracker untouched keeps
        // it at its last-permitted state so live rows are not silently consumed.
        return crate::refs::RefRowDeltaBatch {
            namespace: namespace.to_string(),
            baseline: false,
            rows: Vec::new(),
        };
    }
    if baseline {
        // Forced full re-emit for this namespace: drop its last-emitted map so
        // build_baseline re-seeds every live row as Changed (ADR-0070 #3).
        tracker.reset_namespace(namespace);
        tracker.build_baseline(namespace, source)
    } else {
        tracker.build_incremental(namespace, source)
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
