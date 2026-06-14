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

/// ADR-0055 Rung 2: frame-level epoch identity passed from the kernel to the
/// encoder. Both values come from the `ProjectionManifest` built by
/// `Kernel::projection_manifest()` in `make_update`.
pub(crate) struct FrameEpochStamp {
    /// Within-session monotonic epoch counter (bumped on account-switch etc.).
    pub(crate) snapshot_epoch: u64,
    /// Kernel-start wall-clock ms (`TimingMilestones::started_unix_ms`).
    pub(crate) session_id: u64,
}

/// Encode a snapshot with the typed projection sidecar AND the typed Tier-3
/// envelope fields (ADR-0044). The generic `payload:Value` slot is intentionally
/// left absent (PR-B #991/#979: emission zeroed).
///
/// ADR-0055 Rung 2: `epoch` carries the frame-level epoch identity stamps
/// (`snapshot_epoch` + `session_id`) so old readers ignore them (tail-appended
/// on the wire) while Rung-2 hosts decode and store them for future use.
///
/// ADR-0055 Rung 3 (D3-6): `builder` is the kernel-owned reusable
/// `FlatBufferBuilder`. It is `reset()` at the top of this function so the
/// kernel can hold one builder across ticks and avoid per-tick heap allocation.
/// The returned `UpdateFrameBytes` (`Vec<u8>`) owns its bytes independently —
/// `to_vec()` copies the finished buffer out before this function returns, so
/// the builder buffer is free to reuse on the next tick. No pointer, slice, or
/// FlatBuffers offset into the builder's internal buffer may be retained past
/// this function's return (the builder is single-writer on the actor thread).
///
/// All Rust shells read typed-first; the deprecated `payload` field is absent in
/// the wire bytes. The `snapshot: Value` parameter has been removed — the kernel
/// no longer needs to serialise the full JSON snapshot for transport. The field
/// is retained in the `.fbs` schema (marked `deprecated`) for schema
/// compatibility with old pre-PR-B binaries; new readers never read it.
#[must_use]
pub(crate) fn encode_snapshot_with_envelope(
    builder: &mut FlatBufferBuilder<'_>,
    typed: &[TypedProjectionData],
    envelope: &crate::kernel::KernelSnapshot,
    epoch: &FrameEpochStamp,
) -> UpdateFrameBytes {
    // ADR-0055 Rung 3 (D3-6): reset the reused builder in place of
    // `FlatBufferBuilder::new()`. This preserves the internal heap allocation
    // across ticks (capacity is stable after the first tick warms up), so each
    // 4 Hz encode avoids a fresh heap allocation. The `to_vec()` at the end of
    // this function copies the finished bytes into an owned `Vec<u8>` BEFORE
    // this reset is invoked on the next tick — no caller may retain a borrow
    // into the builder buffer past the return of this function.
    builder.reset();
    let typed_projections = encode_typed_projections(builder, typed);
    let tier3 = envelope.encode_tier3(builder);
    let snapshot = fb::SnapshotFrame::create(
        builder,
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
            negentropy_sync_stats: Some(tier3.negentropy_sync_stats),
            // ADR-0055 Rung 2: stamp frame-level epoch identity (D4). Tail-
            // appended so old readers ignore them (FlatBuffers backward-safety).
            snapshot_epoch: epoch.snapshot_epoch,
            session_id: epoch.session_id,
        },
    );
    let root = fb::UpdateFrame::create(
        builder,
        &fb::UpdateFrameArgs {
            kind: fb::FrameKind::Snapshot,
            snapshot: Some(snapshot),
            panic: None,
        },
    );
    fb::finish_update_frame_buffer(builder, root);
    // ADR-0055 Rung 3 (D3-6): copy the finished bytes OUT of the builder
    // buffer into an owned Vec<u8> BEFORE this function returns. The builder
    // will be reset() on the next encode call; no reference into its internal
    // buffer may survive past here. This is the single ownership transfer point
    // that makes the reuse pattern safe.
    builder.finished_data().to_vec()
}
