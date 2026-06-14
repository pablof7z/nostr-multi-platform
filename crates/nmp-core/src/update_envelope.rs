//! Canonical FlatBuffers update frames for the single kernel→host channel.
//!
//! Every runtime frame is a binary `nmp.transport.UpdateFrame` with file
//! identifier `NMPU`. The frame has exactly two variants:
//!
//! - `Snapshot`: carries the typed Tier-3 envelope fields (ADR-0044) plus the
//!   per-projection typed FlatBuffers sidecar (ADR-0037).
//! - `Panic`: terminal actor-thread death signal.
//!
//! PR-B (#991/#979): the generic `payload:Value` JSON tree is no longer
//! emitted or decoded — the schema field is `(deprecated)` and the generated
//! bindings expose no accessor for it. Consumers read the typed
//! [`SnapshotEnvelope`] (via [`decode_snapshot_envelope`] /
//! [`decode_update_frame`]) and the typed sidecar entries (via
//! [`decode_snapshot_typed_projections`] paired with the per-key decoders in
//! `nmp_core::typed_projections`).

use crate::transport::wire as fb;
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use std::fmt;

// Submodules keep this file under the LOC ceiling; re-exported below.
mod projection_state;
mod relay_status;
mod tier3_frame;
pub use projection_state::WireProjectionState;
pub use relay_status::{RelayStatusEntry, WireSubscriptionEntry};
pub(crate) use tier3_frame::{encode_snapshot_with_envelope, FrameEpochStamp};

/// Schema version of the periodic snapshot payload. Bump on any breaking
/// snapshot field change.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Owned bytes for one FlatBuffers `nmp.transport.UpdateFrame`.
pub type UpdateFrameBytes = Vec<u8>;

/// Typed Tier-3 snapshot envelope — the strongly-typed `SnapshotFrame` fields
/// that Rust consumers (chirp-tui, chirp-desktop) read instead of walking the
/// generic `payload:Value` tree.
///
/// Mirrors the fields written by `encode_snapshot_with_envelope` (ADR-0044).
/// Fields absent from the wire (never-written or default-zero) are returned as
/// `0` / `false` / `None` — same semantics as FlatBuffers native defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapshotEnvelope {
    /// Monotonically-increasing frame revision (from `KernelSnapshot.rev`).
    pub rev: u64,
    /// Kernel schema version (`KERNEL_SCHEMA_VERSION`). Distinct from the
    /// transport `schema_version` field.
    pub kernel_schema_version: u32,
    /// Wall-clock timestamp of this tick (ms since Unix epoch). Actor-liveness
    /// signal (ADR-0028).
    pub last_tick_ms: u64,
    /// Whether the kernel actor is in the running state.
    pub running: bool,
    /// Run-state label (always `"ViewBatch"` today).
    pub update_kind: String,
    // --- Metrics (selected fields used by Rust shells) ---
    /// Total relay events received.
    pub events_rx: u64,
    /// Approximate number of visible timeline items.
    pub visible_items: u64,
    /// Current actor command-queue depth.
    pub actor_queue_depth: u32,
    /// Monotonically-increasing update sequence counter.
    pub update_sequence: u64,
    // --- Relay statuses (PR-B: extended for chirp-desktop typed-first migration) ---
    /// Per-relay connection status rows. Empty when no relays are configured.
    pub relay_statuses: Vec<RelayStatusEntry>,
    /// The singular aggregate connection summary (`relay_status` on the wire).
    /// `None` when the producer wrote no aggregate.
    pub relay_status: Option<RelayStatusEntry>,
    /// Open/closed wire-subscription rows (`wire_subscriptions` on the wire).
    /// Empty when the producer wrote none.
    pub wire_subscriptions: Vec<WireSubscriptionEntry>,
    // --- Error / diagnostic flags ---
    /// Last error toast message, if any.
    pub last_error_toast: Option<String>,
    /// Last error category, if any.
    pub last_error_category: Option<String>,
    /// Last planner error message, if any.
    pub last_planner_error: Option<String>,
    // --- ADR-0055 Rung 2: frame-level epoch identity (D4) ---
    /// Within-session monotonic counter. Bumped on events that invalidate the
    /// host's entire cached projection set (account-switch, schema-change,
    /// explicit resync). On bump → the next frame is a full baseline in Rung 3.
    /// 0 on old (pre-Rung-2) frames (safe: host treats any increase as a bump).
    pub snapshot_epoch: u64,
    /// Kernel-start wall-clock ms (`TimingMilestones::started_unix_ms`). Changes
    /// across process restarts. When the host sees a changed `session_id` it MUST
    /// discard all per-key applied-rev state and re-baseline. 0 on old frames.
    pub session_id: u64,
}

/// Decode the Tier-3 typed `SnapshotFrame` envelope from a FlatBuffers update
/// frame (as produced by `encode_snapshot_with_envelope`).
///
/// Returns `Ok(SnapshotEnvelope)` with whatever tier-3 fields are present in
/// the frame. Fields absent (zero/null by FlatBuffers default) are returned as
/// their Rust zero-value. Returns an error only on a malformed frame that cannot
/// be parsed at all.
///
/// This is the Rust counterpart to iOS's `TypedSnapshotEnvelope` (used by
/// `KernelUpdateFrameDecoder.extractTypedEnvelope`). Rust shells that complete
/// the PR-B typed-first migration use this function instead of decoding
/// `payload:Value`.
pub fn decode_snapshot_envelope(bytes: &[u8]) -> Result<SnapshotEnvelope, UpdateFrameDecodeError> {
    if !fb::update_frame_buffer_has_identifier(bytes) {
        return Err(UpdateFrameDecodeError::InvalidFlatbuffer(
            "missing NMPU file identifier".to_string(),
        ));
    }
    let frame = fb::root_as_update_frame(bytes)
        .map_err(|err| UpdateFrameDecodeError::InvalidFlatbuffer(format!("{err:?}")))?;
    if frame.kind() != fb::FrameKind::Snapshot {
        return Err(UpdateFrameDecodeError::MissingSnapshotPayload);
    }
    let snapshot = frame
        .snapshot()
        .ok_or(UpdateFrameDecodeError::MissingSnapshotPayload)?;
    Ok(envelope_from_snapshot_frame(&snapshot))
}

/// Build an owned [`SnapshotEnvelope`] from a decoded `SnapshotFrame` table.
/// Shared by [`decode_snapshot_envelope`] and [`decode_update_frame`] so the
/// two public decode paths cannot drift.
fn envelope_from_snapshot_frame(snapshot: &fb::SnapshotFrame<'_>) -> SnapshotEnvelope {
    let (events_rx, visible_items, actor_queue_depth, update_sequence) =
        if let Some(metrics) = snapshot.metrics() {
            (
                metrics.events_rx(),
                metrics.visible_items(),
                metrics.actor_queue_depth(),
                metrics.update_sequence(),
            )
        } else {
            (0, 0, 0, 0)
        };

    SnapshotEnvelope {
        rev: snapshot.rev(),
        kernel_schema_version: snapshot.kernel_schema_version(),
        last_tick_ms: snapshot.last_tick_ms(),
        running: snapshot.running(),
        update_kind: snapshot.update_kind().unwrap_or("").to_string(),
        events_rx,
        visible_items,
        actor_queue_depth,
        update_sequence,
        relay_statuses: relay_status::decode_relay_statuses(snapshot),
        relay_status: relay_status::decode_relay_status_aggregate(snapshot),
        wire_subscriptions: relay_status::decode_wire_subscriptions(snapshot),
        last_error_toast: snapshot.last_error_toast().map(str::to_string),
        last_error_category: snapshot.last_error_category().map(str::to_string),
        last_planner_error: snapshot.last_planner_error().map(str::to_string),
        // ADR-0055 Rung 2: decode frame-level epoch identity (D4). Old
        // (pre-Rung-2) frames return 0 for both (FlatBuffers default) — safe.
        snapshot_epoch: snapshot.snapshot_epoch(),
        session_id: snapshot.session_id(),
    }
}

/// Decode only the typed-projection sidecar from a FlatBuffers update frame.
///
/// Pair with [`decode_snapshot_envelope`] for the Tier-3 envelope fields and
/// with the per-key decoders in `nmp_core::typed_projections` to interpret
/// each entry's opaque payload bytes.
pub fn decode_snapshot_typed_projections(
    bytes: &[u8],
) -> Result<Vec<TypedProjectionData>, UpdateFrameDecodeError> {
    if !fb::update_frame_buffer_has_identifier(bytes) {
        return Err(UpdateFrameDecodeError::InvalidFlatbuffer(
            "missing NMPU file identifier".to_string(),
        ));
    }
    let frame = fb::root_as_update_frame(bytes)
        .map_err(|err| UpdateFrameDecodeError::InvalidFlatbuffer(format!("{err:?}")))?;
    if frame.kind() != fb::FrameKind::Snapshot {
        return Err(UpdateFrameDecodeError::MissingSnapshotPayload);
    }
    let snapshot = frame
        .snapshot()
        .ok_or(UpdateFrameDecodeError::MissingSnapshotPayload)?;
    decode_typed_projections(&snapshot)
}

/// Actor-thread death payload. Terminal: hosts must stop sending commands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PanicFrame {
    pub msg: String,
}

/// Decoded view used by Rust consumers and tests. Runtime transport remains
/// FlatBuffers bytes; this enum is not the wire shape.
///
/// PR-B (#991/#979): `Snapshot` carries the typed [`SnapshotEnvelope`]
/// (Tier-3 fields) instead of the deleted generic JSON `Value` payload.
/// Projection data travels in the typed sidecar — pair with
/// [`decode_snapshot_typed_projections`].
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateEnvelope {
    Snapshot(SnapshotEnvelope),
    Panic(PanicFrame),
}

/// Owned, decoded form of one `nmp.transport.TypedProjection` sidecar entry.
///
/// The `payload` is opaque to `nmp-core`: it is a host-declared, framework-side
/// FlatBuffers buffer identified by `schema_id` / `schema_version` /
/// `file_identifier`. The transport layer never interprets these bytes; it only
/// carries them losslessly alongside the generic `Value` snapshot.
///
/// ADR-0055 Rung 2: `projection_rev` and `state` fields added (tail-appended on
/// the wire — old decoders read them as default 0 / Changed, treating every entry
/// as a payload update).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypedProjectionData {
    /// Projection key (host-declared identity of this projection).
    pub key: String,
    /// Stable schema identifier for the typed payload.
    pub schema_id: String,
    /// Schema version of the typed payload. Defaults to `1` on the wire.
    pub schema_version: u32,
    /// FlatBuffers file identifier of the typed payload, if any.
    pub file_identifier: String,
    /// Opaque typed payload bytes, carried verbatim by the transport.
    pub payload: Vec<u8>,
    /// ADR-0055 Rung 2: monotonic revision for this projection key, derived
    /// by the kernel's `ProjectionRevTracker` (SUM of source-version counters
    /// across the key's declared dependencies). Hosts store this alongside the
    /// decoded value and use it in Rung 3 to skip re-decode on unchanged keys.
    /// 0 on old (pre-Rung-2) frames.
    pub projection_rev: u64,
    /// ADR-0055 Rung 2: presence classification for this tick. Hosts decode and
    /// store this value; no behavior change in Rung 2 (all projections are still
    /// emitted). In Rung 3 `Cleared` tells the host to drop its cached value and
    /// `Changed` tells it to decode `payload` and update the cache.
    /// Default `Changed` on old (pre-Rung-2) frames.
    pub state: WireProjectionState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateFrameDecodeError {
    InvalidFlatbuffer(String),
    InvalidValue(String),
    MissingSnapshotPayload,
    MissingPanicPayload,
    UnexpectedPanicFrame(String),
}

impl fmt::Display for UpdateFrameDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFlatbuffer(msg) => write!(f, "invalid update frame: {msg}"),
            Self::InvalidValue(msg) => write!(f, "invalid update value: {msg}"),
            Self::MissingSnapshotPayload => write!(f, "snapshot frame missing payload"),
            Self::MissingPanicPayload => write!(f, "panic frame missing payload"),
            Self::UnexpectedPanicFrame(msg) => write!(f, "expected snapshot, got panic: {msg}"),
        }
    }
}

impl std::error::Error for UpdateFrameDecodeError {}

/// Encode a snapshot frame from a typed [`SnapshotEnvelope`] plus an optional
/// typed-projection sidecar.
///
/// This is the auxiliary-producer encoder: non-kernel producers (`nmp-wasm`'s
/// browser runtime, test fixtures, benches) build a [`SnapshotEnvelope`] and
/// encode it here. The production kernel path is
/// `encode_snapshot_with_envelope` (tier3_frame.rs), which writes the FULL
/// Tier-3 field set straight off `KernelSnapshot`; this function writes the
/// [`SnapshotEnvelope`] subset (the documented Rust consumer surface), so a
/// frame round-trips losslessly through [`decode_snapshot_envelope`].
///
/// No generic `payload:Value` is written — the field is `(deprecated)` in the
/// schema and the generated bindings expose no writer for it (PR-B #991/#979).
#[must_use]
pub fn encode_snapshot_frame(
    envelope: &SnapshotEnvelope,
    typed: &[TypedProjectionData],
) -> UpdateFrameBytes {
    let mut builder = FlatBufferBuilder::new();
    let typed_projections = encode_typed_projections(&mut builder, typed);
    let update_kind = builder.create_string(&envelope.update_kind);
    let relay_status = envelope
        .relay_status
        .as_ref()
        .map(|entry| relay_status::encode_relay_status_entry(&mut builder, entry));
    let relay_statuses =
        relay_status::encode_relay_statuses(&mut builder, &envelope.relay_statuses);
    let wire_subscriptions =
        relay_status::encode_wire_subscriptions(&mut builder, &envelope.wire_subscriptions);
    let metrics = fb::Metrics::create(
        &mut builder,
        &fb::MetricsArgs {
            events_rx: envelope.events_rx,
            visible_items: envelope.visible_items,
            actor_queue_depth: envelope.actor_queue_depth,
            update_sequence: envelope.update_sequence,
            ..Default::default()
        },
    );
    let last_error_toast =
        envelope.last_error_toast.as_deref().map(|s| builder.create_string(s));
    let last_error_category =
        envelope.last_error_category.as_deref().map(|s| builder.create_string(s));
    let last_planner_error =
        envelope.last_planner_error.as_deref().map(|s| builder.create_string(s));
    let snapshot = fb::SnapshotFrame::create(
        &mut builder,
        &fb::SnapshotFrameArgs {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            typed_projections,
            rev: envelope.rev,
            kernel_schema_version: envelope.kernel_schema_version,
            last_tick_ms: envelope.last_tick_ms,
            update_kind: Some(update_kind),
            running: envelope.running,
            metrics: Some(metrics),
            relay_status,
            relay_statuses,
            wire_subscriptions,
            last_error_toast,
            last_error_category,
            last_planner_error,
            // ADR-0055 Rung 2: stamp frame-level epoch identity (D4).
            snapshot_epoch: envelope.snapshot_epoch,
            session_id: envelope.session_id,
            ..Default::default()
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

/// Build the `typed_projections` vector, returning `None` when there are no
/// entries so the optional FlatBuffers slot is omitted entirely (wire-stable).
fn encode_typed_projections<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    typed: &[TypedProjectionData],
) -> Option<
    WIPOffset<flatbuffers::Vector<'bldr, flatbuffers::ForwardsUOffset<fb::TypedProjection<'bldr>>>>,
> {
    if typed.is_empty() {
        return None;
    }
    let offsets: Vec<_> = typed
        .iter()
        .map(|entry| {
            let schema_id = builder.create_string(&entry.schema_id);
            let file_identifier = builder.create_string(&entry.file_identifier);
            let payload = builder.create_vector(&entry.payload);
            let typed_payload = fb::TypedPayload::create(
                builder,
                &fb::TypedPayloadArgs {
                    schema_id: Some(schema_id),
                    schema_version: entry.schema_version,
                    file_identifier: Some(file_identifier),
                    payload: Some(payload),
                },
            );
            let key = builder.create_string(&entry.key);
            fb::TypedProjection::create(
                builder,
                &fb::TypedProjectionArgs {
                    key: Some(key),
                    payload: Some(typed_payload),
                    // ADR-0055 Rung 2: stamp per-projection rev + state.
                    projection_rev: entry.projection_rev,
                    state: entry.state.into(),
                },
            )
        })
        .collect();
    Some(builder.create_vector(&offsets))
}

/// Encode the terminal actor-death signal as one FlatBuffers update frame.
#[must_use]
pub fn encode_panic(msg: impl Into<String>) -> UpdateFrameBytes {
    let mut builder = FlatBufferBuilder::new();
    let msg = builder.create_string(&msg.into());
    let panic = fb::PanicFrame::create(&mut builder, &fb::PanicFrameArgs { msg: Some(msg) });
    let root = fb::UpdateFrame::create(
        &mut builder,
        &fb::UpdateFrameArgs {
            kind: fb::FrameKind::Panic,
            snapshot: None,
            panic: Some(panic),
        },
    );
    fb::finish_update_frame_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Decode one update frame into the canonical discriminated envelope
/// (ADR-0001 / T103): the FlatBuffers `FrameKind` tag IS the discriminant.
///
/// PR-B (#991/#979): the `Snapshot` arm carries the typed
/// [`SnapshotEnvelope`]; the generic JSON payload no longer exists on the
/// wire. Pre-PR-B frames (payload-only, no Tier-3 fields) still parse — their
/// Tier-3 fields read as FlatBuffers defaults (zero/empty).
pub fn decode_update_frame(bytes: &[u8]) -> Result<UpdateEnvelope, UpdateFrameDecodeError> {
    if !fb::update_frame_buffer_has_identifier(bytes) {
        return Err(UpdateFrameDecodeError::InvalidFlatbuffer(
            "missing NMPU file identifier".to_string(),
        ));
    }
    let frame = fb::root_as_update_frame(bytes)
        .map_err(|err| UpdateFrameDecodeError::InvalidFlatbuffer(format!("{err:?}")))?;
    match frame.kind() {
        kind if kind == fb::FrameKind::Snapshot => {
            let snapshot = frame
                .snapshot()
                .ok_or(UpdateFrameDecodeError::MissingSnapshotPayload)?;
            Ok(UpdateEnvelope::Snapshot(envelope_from_snapshot_frame(
                &snapshot,
            )))
        }
        kind if kind == fb::FrameKind::Panic => {
            let panic = frame
                .panic()
                .ok_or(UpdateFrameDecodeError::MissingPanicPayload)?;
            Ok(UpdateEnvelope::Panic(PanicFrame {
                msg: panic.msg().to_string(),
            }))
        }
        other => Err(UpdateFrameDecodeError::InvalidFlatbuffer(format!(
            "unknown frame kind {}",
            other.0
        ))),
    }
}

fn decode_typed_projections(
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
            // ADR-0055 Rung 2: decode rev + state. Old (pre-Rung-2) writers
            // return 0 / Changed (FlatBuffers defaults) — correct: treat as
            // a payload update at rev 0.
            projection_rev: projection.projection_rev(),
            state: projection.state().into(),
        });
    }
    Ok(out)
}


/// Best-effort message extraction from a `catch_unwind` payload.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "unknown panic in actor thread".to_string()
    }
}

#[cfg(test)]
mod tests;
