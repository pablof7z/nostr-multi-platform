//! `ChangedProjectionKey` / `decode_snapshot_changed_projection_keys` — #3131.
//!
//! Split out of `update_envelope.rs` to keep that file within the 500-LOC
//! ceiling (AGENTS.md). The off-actor-thread update-frame observer
//! (`nmp-native-runtime`'s `notify_update_frame_observers`) fires
//! unconditionally on every emitted frame, so its payload must be cheap to
//! build: this decodes ONLY each sidecar entry's `key` / `state`, never its
//! payload bytes (contrast [`super::decode_snapshot_typed_projections`],
//! which clones every entry's payload and is meant for occasional,
//! consumer-driven reads).

use super::WireProjectionState;
use crate::transport::wire as fb;

/// Cheap key-only summary of one typed-projection sidecar entry — #3131.
///
/// Carries none of [`super::TypedProjectionData`]'s payload/schema fields, so
/// building a list of these does not clone payload bytes. See
/// [`decode_snapshot_changed_projection_keys`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangedProjectionKey {
    /// Projection key (host-declared identity of this projection).
    pub key: String,
    /// ADR-0070 Rung 2 presence classification for this tick — `Changed`
    /// (payload updated) or `Cleared` (host must drop its cached value).
    pub state: WireProjectionState,
}

/// Decode only the changed-projection-key summary from a FlatBuffers update
/// frame — #3131.
///
/// Cheaper than [`super::decode_snapshot_typed_projections`]: skips every
/// entry's opaque payload bytes (and `schema_id` / `file_identifier`
/// strings), so it is safe to call unconditionally on every emitted frame,
/// e.g. from `nmp-native-runtime`'s off-actor-thread update-frame observer.
/// Returns an empty vec (not an error) for non-`Snapshot` frames (e.g.
/// `Panic`), since "no projections changed" is the correct summary for a
/// frame with no projection data.
pub fn decode_snapshot_changed_projection_keys(bytes: &[u8]) -> Vec<ChangedProjectionKey> {
    let Ok(frame) = fb::root_as_update_frame(bytes) else {
        return Vec::new();
    };
    if frame.kind() != fb::FrameKind::Snapshot {
        return Vec::new();
    }
    let Some(snapshot) = frame.snapshot() else {
        return Vec::new();
    };
    super::typed_projection_decode::decode_typed_projection_keys(&snapshot)
}
