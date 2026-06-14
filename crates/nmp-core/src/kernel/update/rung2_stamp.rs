//! ADR-0055 Rung 2 — wire-contract stamping helpers.
//!
//! Extracted from `update::make_update` so the hot emit path stays within the
//! 500-LOC file ceiling (mirrors how Rung 1 siblinged `kernel_impl.rs` /
//! `kernel_oracle.rs`). These are pure transforms over the per-tick
//! `ProjectionManifest`; they do NOT touch kernel state except the explicit
//! `record_emitted` baseline advance.

use crate::kernel::projection_rev::{ProjectionManifest, ProjectionPresence, ProjectionRevTracker};
use crate::update_envelope::{FrameEpochStamp, TypedProjectionData, WireProjectionState};

/// Build the frame-level epoch stamp (`snapshot_epoch` + `session_id`) from the
/// per-tick manifest. The host detects a new kernel run when `session_id`
/// changes and a full baseline when `snapshot_epoch` changes (ADR-0055 D4).
#[must_use]
pub(super) fn epoch_stamp(manifest: &ProjectionManifest) -> FrameEpochStamp {
    FrameEpochStamp {
        snapshot_epoch: manifest.epoch,
        session_id: manifest.session_id,
    }
}

/// Stamp each emitted `TypedProjectionData` with its `projection_rev` and
/// `state` from the manifest. Every projection is still emitted every tick (no
/// omission — that is Rung 3). For Rung 2 only `Changed` and `Cleared` appear:
/// `Unchanged` is a Rung-3 concept (absence = Unchanged, never emitted). Both
/// `Changed` and `Unchanged` map to `Changed` on the wire because all
/// projections are emitted regardless of presence.
///
/// Host-registered projections (keys not in `KERNEL_BUILTIN_PROJECTION_KEYS`)
/// are absent from the manifest and keep their defaults (rev 0 / `Changed`)
/// because the manifest covers Tier-2 built-ins only — they are unconditionally
/// `Changed` at every tick (no host-projection manifests yet).
///
/// Note: `record_emitted` is NOT called here. In test/test-support builds the
/// oracle (`run_projection_oracle` → `oracle.record_tick`) advances the baseline
/// AFTER its assertion so it sees the pre-emit tracker state; in production
/// [`record_emitted_for_manifest`] is called after the encode.
#[must_use]
pub(super) fn stamp_typed_projections(
    typed: Vec<TypedProjectionData>,
    manifest: &ProjectionManifest,
) -> Vec<TypedProjectionData> {
    typed
        .into_iter()
        .map(|mut entry| {
            if let Some(ps) = manifest.states.iter().find(|s| s.key == entry.key.as_str()) {
                entry.projection_rev = ps.rev;
                entry.state = match ps.presence {
                    ProjectionPresence::Cleared => WireProjectionState::Cleared,
                    _ => WireProjectionState::Changed,
                };
            }
            entry
        })
        .collect()
}

/// Advance the tracker's last-emitted baseline for every Tier-2 built-in so the
/// NEXT tick's presence computation is accurate. Production-only: in
/// test/test-support builds the oracle does this AFTER its check (the oracle
/// MUST run before `record_emitted` so it sees the pre-emit tracker state).
pub(super) fn record_emitted_for_manifest(
    tracker: &mut ProjectionRevTracker,
    manifest: &ProjectionManifest,
) {
    for ps in &manifest.states {
        tracker.record_emitted(ps.key);
    }
}
