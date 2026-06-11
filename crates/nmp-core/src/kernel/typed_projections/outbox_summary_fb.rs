//! Typed FlatBuffers wire codec for the kernel-owned `"outbox_summary"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"outbox_summary"`: the serialisation of `outbox_summary_snapshot()`, an
//! [`OutboxSummarySnapshot`](crate::kernel::OutboxSummarySnapshot)
//! (per-status counters + the pre-formatted English `title` / `subtitle`
//! strings). This module adds a **typed FlatBuffers** encoding of the same
//! shape, carried in the `typed_projections` sidecar (ADR-0037) ALONGSIDE —
//! never replacing — the generic `Value` projection.
//!
//! [`OutboxSummaryModel`] is built directly from the same summary struct the
//! JSON path serialises (mapped inline in
//! [`Kernel::builtin_typed_projections`](crate::kernel::Kernel), where the
//! `pub(super)` DTO type is nameable), in the same tick, so the two wire forms
//! cannot diverge.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input.

// The generated FlatBuffers bindings are intrinsically `unsafe`. This `allow`
// block scopes the relaxation to the single generated module.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/outbox_summary_generated.rs"]
pub mod generated;

use flatbuffers::FlatBufferBuilder;

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const OUTBOX_SUMMARY_SCHEMA_ID: &str = "outbox_summary";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const OUTBOX_SUMMARY_FILE_IDENTIFIER: &[u8; 4] = b"KOXS";
/// Wire schema version. Bump on any breaking change to `outbox_summary.fbs`.
pub const OUTBOX_SUMMARY_SCHEMA_VERSION: u32 = 1;

/// The `"outbox_summary"` read model — a field-for-field mirror of the
/// SERIALISED [`OutboxSummarySnapshot`](crate::kernel::OutboxSummarySnapshot).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutboxSummaryModel {
    pub title: String,
    pub subtitle: String,
    pub total: u32,
    pub sending: u32,
    pub retrying: u32,
    pub queued: u32,
    pub failed: u32,
}

// --- encode ---------------------------------------------------------------

/// Encode an [`OutboxSummaryModel`] to typed FlatBuffers bytes (with the `KOXS`
/// file identifier).
#[must_use]
pub(crate) fn encode_outbox_summary(model: &OutboxSummaryModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let title = fbb.create_string(&model.title);
    let subtitle = fbb.create_string(&model.subtitle);
    let root = fb::OutboxSummarySnapshot::create(
        &mut fbb,
        &fb::OutboxSummarySnapshotArgs {
            title: Some(title),
            subtitle: Some(subtitle),
            total: model.total,
            sending: model.sending,
            retrying: model.retrying,
            queued: model.queued,
            failed: model.failed,
        },
    );
    fb::finish_outbox_summary_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_outbox_summary`])
/// back into an [`OutboxSummaryModel`]. Returns an error string on any
/// malformed input.
pub fn decode_outbox_summary(bytes: &[u8]) -> Result<OutboxSummaryModel, String> {
    if bytes.len() < 8 || !fb::outbox_summary_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KOXS file identifier".to_string());
    }
    let root = fb::root_as_outbox_summary_snapshot(bytes)
        .map_err(|e| format!("not a valid OutboxSummarySnapshot buffer: {e}"))?;

    Ok(OutboxSummaryModel {
        title: root.title().unwrap_or_default().to_string(),
        subtitle: root.subtitle().unwrap_or_default().to_string(),
        total: root.total(),
        sending: root.sending(),
        retrying: root.retrying(),
        queued: root.queued(),
        failed: root.failed(),
    })
}

#[cfg(test)]
#[path = "outbox_summary_fb_tests.rs"]
mod tests;
