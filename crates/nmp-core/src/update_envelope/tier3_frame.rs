//! ADR-0044 — the dual-emit `SnapshotFrame` encoder that carries the typed
//! Tier-3 envelope fields alongside the generic JSON `payload`.
//!
//! Split out of `update_envelope.rs` to keep that file under the LOC ceiling.
//! The actual per-field offset population lives in
//! `crate::kernel::KernelSnapshot::encode_tier3` (where the struct fields are
//! visible); this module owns only the assembly of the final `SnapshotFrame`
//! table — the transport layer's `SnapshotFrame` shape.

use super::{
    encode_typed_projections, encode_value, TypedProjectionData, UpdateFrameBytes,
    SNAPSHOT_SCHEMA_VERSION,
};
use crate::transport::wire as fb;
use flatbuffers::FlatBufferBuilder;
use serde_json::Value;

/// Encode a snapshot with the typed projection sidecar AND the typed Tier-3
/// envelope fields (ADR-0044).
///
/// Additive over [`super::encode_snapshot_with_typed`]: it writes the same
/// generic `payload` Value and `typed_projections` sidecar, then *also*
/// populates the first-class typed `SnapshotFrame` envelope fields (`rev`,
/// `running`, `metrics`, `relay_statuses`, …) directly from the
/// `KernelSnapshot` struct — not by re-walking the JSON tree. The two
/// representations are emitted in parallel (dual-emit, per ADR-0037 Commitment
/// 4 applied to Tier-3): an un-migrated host keeps reading `payload`; a
/// migrated host prefers the typed fields. The kernel is the only caller; the
/// `KernelSnapshot` type is crate-private.
#[must_use]
pub(crate) fn encode_snapshot_with_envelope(
    snapshot: Value,
    typed: &[TypedProjectionData],
    envelope: &crate::kernel::KernelSnapshot,
) -> UpdateFrameBytes {
    let mut builder = FlatBufferBuilder::new();
    let payload = encode_value(&mut builder, &snapshot);
    let typed_projections = encode_typed_projections(&mut builder, typed);
    let tier3 = envelope.encode_tier3(&mut builder);
    let snapshot = fb::SnapshotFrame::create(
        &mut builder,
        &fb::SnapshotFrameArgs {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            payload: Some(payload),
            typed_projections,
            rev: tier3.rev,
            kernel_schema_version: tier3.kernel_schema_version,
            last_tick_ms: tier3.last_tick_ms,
            update_kind: Some(tier3.update_kind),
            running: tier3.running,
            metrics: Some(tier3.metrics),
            relay_status: Some(tier3.relay_status),
            relay_statuses: Some(tier3.relay_statuses),
            logical_interests: Some(tier3.logical_interests),
            wire_subscriptions: Some(tier3.wire_subscriptions),
            logs: Some(tier3.logs),
            last_error_toast: tier3.last_error_toast,
            last_error_category: tier3.last_error_category,
            last_planner_error: tier3.last_planner_error,
            store_open_failure: tier3.store_open_failure,
            no_configured_relays: tier3.no_configured_relays,
        },
    );
    let root = fb::UpdateFrame::create(
        &mut builder,
        &fb::UpdateFrameArgs {
            kind: fb::FrameKind::Snapshot,
            snapshot: Some(snapshot),
            panic: None,
        },
    );
    fb::finish_update_frame_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}
