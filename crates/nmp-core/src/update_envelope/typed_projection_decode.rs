//! Typed-projection sidecar decode (ADR-0072 / ADR-0070 Rung 2).
//!
//! Extracted from `update_envelope.rs` per ADR-0070 §5 to keep the parent file
//! under the 500-LOC hard ceiling. Decodes the per-projection FlatBuffers
//! sidecar entries (`typed_projections`) carried on a `SnapshotFrame` into the
//! owned [`TypedProjectionData`] rows that Rust consumers and the
//! ProjectionCache merge operate on.

use super::{ChangedProjectionKey, TypedProjectionData, UpdateFrameDecodeError};
use crate::transport::wire as fb;

/// Decode the typed-projection sidecar rows from a snapshot frame.
///
/// Returns an empty vec when the frame carries no `typed_projections` vector.
/// Each row preserves the on-wire `projection_rev` and `state`
/// (Changed/Cleared) so the host-side ProjectionCache can apply the ADR-0070
/// Rung 3 incremental merge. Pre-Rung-2 writers return `0` / `Changed`
/// (FlatBuffers defaults) — correctly interpreted as a rev-0 payload update.
pub(super) fn decode_typed_projections(
    snapshot: &fb::SnapshotFrame<'_>,
) -> Result<Vec<TypedProjectionData>, UpdateFrameDecodeError> {
    let Some(projections) = snapshot.typed_projections() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(projections.len());
    for index in 0..projections.len() {
        let projection = projections.get(index);
        let key = projection
            .key()
            .ok_or_else(|| {
                UpdateFrameDecodeError::InvalidValue(format!(
                    "typed projection at index {index} missing key"
                ))
            })?
            .to_string();
        let typed = projection.payload().ok_or_else(|| {
            UpdateFrameDecodeError::InvalidValue(format!(
                "typed projection {key:?} missing payload"
            ))
        })?;
        let payload = typed
            .payload()
            .map(|bytes| bytes.bytes().to_vec())
            .unwrap_or_default();
        out.push(TypedProjectionData {
            key,
            schema_id: typed.schema_id().unwrap_or_default().to_string(),
            schema_version: typed.schema_version(),
            file_identifier: typed.file_identifier().unwrap_or_default().to_string(),
            payload,
            // ADR-0070 Rung 2: decode rev + state. Old (pre-Rung-2) writers
            // return 0 / Changed (FlatBuffers defaults) — correct: treat as
            // a payload update at rev 0.
            projection_rev: projection.projection_rev(),
            state: projection.state().into(),
        });
    }
    Ok(out)
}

/// Decode only the `key` / `state` pair for each typed-projection sidecar
/// entry — #3131. Deliberately skips `projection.payload()` (the nested
/// `TypedPayload` table and its byte vector) entirely, so this is cheap
/// enough to run unconditionally on every emitted frame: no payload-byte
/// clone, no `schema_id` / `file_identifier` string allocation.
///
/// Because ADR-0070 Rung 3 already omits unchanged keys from the wire frame
/// (see `rung3_baseline_tests.rs`), the returned list IS the set of
/// projection keys that changed (or cleared) on this frame — not a filtered
/// view of a larger "all projections" list.
pub(super) fn decode_typed_projection_keys(
    snapshot: &fb::SnapshotFrame<'_>,
) -> Vec<ChangedProjectionKey> {
    let Some(projections) = snapshot.typed_projections() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(projections.len());
    for index in 0..projections.len() {
        let projection = projections.get(index);
        let Some(key) = projection.key() else {
            continue;
        };
        out.push(ChangedProjectionKey {
            key: key.to_string(),
            state: projection.state().into(),
        });
    }
    out
}
