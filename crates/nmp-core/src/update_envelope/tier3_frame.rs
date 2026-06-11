//! ADR-0044 — the Tier-3 `SnapshotFrame` encoder that carries the typed
//! projection sidecar and the typed Tier-3 envelope fields.
//!
//! Split out of `update_envelope.rs` to keep that file under the LOC ceiling.
//! The actual per-field offset population lives in
//! `crate::kernel::KernelSnapshot::encode_tier3` (where the struct fields are
//! visible); this module owns only the assembly of the final `SnapshotFrame`
//! table — the transport layer's `SnapshotFrame` shape.
//!
//! PR-B (#991/#979): the `payload:Value` slot is now set to `None`. The
//! generic JSON Value tree is no longer emitted. Every Rust shell (chirp-tui,
//! chirp-desktop, nmp-gallery TUI, nmp-gallery desktop) reads typed-first from
//! the Tier-3 `SnapshotEnvelope` + per-projection typed sidecars.
//! iOS is unaffected — `KernelUpdateFrameDecoder.swift` already reads the Tier-3
//! envelope fields and never read `payload`.
//! Android was BROKEN by PR-B (#1084): `KernelUpdateFrameDecoder.kt` gated its
//! entire decode on `snapshot.payload ?: return null`; the fix rebuilds the
//! Android spine from the Tier-3 fields (same as iOS) in the same PR.
//! Web/TS still reads `payload` on the generic path and is unaffected until
//! its typed-first port (#1007, post-v1).

use super::{
    encode_typed_projections, TypedProjectionData, UpdateFrameBytes,
    SNAPSHOT_SCHEMA_VERSION,
};
use crate::transport::wire as fb;
use flatbuffers::FlatBufferBuilder;

/// Encode a snapshot with the typed projection sidecar AND the typed Tier-3
/// envelope fields (ADR-0044). The generic `payload:Value` slot is intentionally
/// left absent (PR-B #991/#979: emission zeroed).
///
/// All Rust shells read typed-first; the deprecated `payload` field is absent in
/// the wire bytes. The `snapshot: Value` parameter has been removed — the kernel
/// no longer needs to serialise the full JSON snapshot for transport. The field
/// is retained in the `.fbs` schema (marked `deprecated`) for schema
/// compatibility with old pre-PR-B binaries; new readers never read it.
#[must_use]
pub(crate) fn encode_snapshot_with_envelope(
    typed: &[TypedProjectionData],
    envelope: &crate::kernel::KernelSnapshot,
) -> UpdateFrameBytes {
    let mut builder = FlatBufferBuilder::new();
    let typed_projections = encode_typed_projections(&mut builder, typed);
    let tier3 = envelope.encode_tier3(&mut builder);
    let snapshot = fb::SnapshotFrame::create(
        &mut builder,
        &fb::SnapshotFrameArgs {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            // PR-B: the deprecated `payload:Value` slot no longer exists in
            // the regenerated bindings — zeroing is compile-time guaranteed.
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
