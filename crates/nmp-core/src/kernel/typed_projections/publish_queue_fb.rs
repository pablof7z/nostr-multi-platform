//! Typed FlatBuffers wire codec for the kernel-owned `"publish_queue"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"publish_queue"`: the serialisation of `publish_queue_snapshot()`, a slice
//! of [`PublishQueueEntry`](crate::kernel::PublishQueueEntry) (each owning a
//! `Vec<RelayAckOutcome>`). This module adds a **typed FlatBuffers** encoding of
//! the same shape, carried in the `typed_projections` sidecar (ADR-0037)
//! ALONGSIDE — never replacing — the generic `Value` projection.
//!
//! [`PublishQueueModel`] is built directly from the same entry slice the JSON
//! path serialises (mapped inline in
//! [`Kernel::builtin_typed_projections`](crate::kernel::Kernel), where the
//! `pub(super)`/`pub(crate)` DTO types are nameable), in the same tick, so the
//! two wire forms cannot structurally diverge. The two `#[serde(skip)]` DTO
//! fields (`signed_event`, `target`) never cross the wire and are intentionally
//! absent from the model.
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
#[path = "generated/publish_queue_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const PUBLISH_QUEUE_SCHEMA_ID: &str = "publish_queue";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const PUBLISH_QUEUE_FILE_IDENTIFIER: &[u8; 4] = b"KPBQ";
/// Wire schema version. Bump on any breaking change to `publish_queue.fbs`.
pub const PUBLISH_QUEUE_SCHEMA_VERSION: u32 = 1;

/// One relay's terminal verdict — a field-for-field mirror of the SERIALISED
/// [`RelayAckOutcome`](crate::kernel::RelayAckOutcome).
///
/// Public surface (re-exported via `nmp_core::typed_projections`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RelayAckOutcomeRow {
    /// Relay URL this verdict came from.
    pub relay_url: String,
    /// Terminal status for this relay (e.g. `"ok"`, `"failed"`).
    pub status: String,
    /// Human-readable message from the relay, if any.
    pub message: String,
    /// Machine reason code from the relay, if any.
    pub relay_reason: String,
}

/// One settled/in-flight publish row — a field-for-field mirror of the
/// SERIALISED [`PublishQueueEntry`](crate::kernel::PublishQueueEntry).
///
/// Public surface (re-exported via `nmp_core::typed_projections`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PublishQueueEntryRow {
    /// Event id of the publish (empty until signed).
    pub event_id: String,
    /// Nostr event kind (opaque `uint` passthrough — no NIP semantics).
    pub kind: u32,
    /// Pre-formatted human title for this row.
    pub title: String,
    /// Number of relays this publish targets.
    pub target_relays: u32,
    /// Aggregate status string for the row.
    pub status: String,
    /// Whether a retry is offered for this row.
    pub can_retry: bool,
    /// Per-relay terminal verdicts collected so far.
    pub relay_outcomes: Vec<RelayAckOutcomeRow>,
}

/// The `"publish_queue"` read model — the ordered queue rows. Built from the
/// same `PublishQueueEntry` slice the JSON projection serialises (mapped inline
/// in [`Kernel::builtin_typed_projections`](crate::kernel::Kernel)).
///
/// Public surface (re-exported via `nmp_core::typed_projections`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PublishQueueModel {
    /// Publish queue rows in producer order.
    pub entries: Vec<PublishQueueEntryRow>,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`PublishQueueModel`] to typed FlatBuffers bytes (with the `KPBQ`
/// file identifier). Row order is preserved verbatim.
#[must_use]
pub(crate) fn encode_publish_queue(model: &PublishQueueModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let entry_offsets: Vec<WIPOffset<fb::PublishQueueEntry>> = model
        .entries
        .iter()
        .map(|entry| {
            let outcome_offsets: Vec<WIPOffset<fb::RelayAckOutcome>> = entry
                .relay_outcomes
                .iter()
                .map(|outcome| {
                    let relay_url = fbb.create_string(&outcome.relay_url);
                    let status = fbb.create_string(&outcome.status);
                    let message = fbb.create_string(&outcome.message);
                    let relay_reason = fbb.create_string(&outcome.relay_reason);
                    fb::RelayAckOutcome::create(
                        &mut fbb,
                        &fb::RelayAckOutcomeArgs {
                            relay_url: Some(relay_url),
                            status: Some(status),
                            message: Some(message),
                            relay_reason: Some(relay_reason),
                        },
                    )
                })
                .collect();
            let relay_outcomes = fbb.create_vector(&outcome_offsets);

            let event_id = fbb.create_string(&entry.event_id);
            let title = fbb.create_string(&entry.title);
            let status = fbb.create_string(&entry.status);
            fb::PublishQueueEntry::create(
                &mut fbb,
                &fb::PublishQueueEntryArgs {
                    event_id: Some(event_id),
                    kind: entry.kind,
                    title: Some(title),
                    target_relays: entry.target_relays,
                    status: Some(status),
                    can_retry: entry.can_retry,
                    relay_outcomes: Some(relay_outcomes),
                },
            )
        })
        .collect();
    let entries = fbb.create_vector(&entry_offsets);

    let root = fb::PublishQueueSnapshot::create(
        &mut fbb,
        &fb::PublishQueueSnapshotArgs {
            entries: Some(entries),
        },
    );
    fb::finish_publish_queue_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_publish_queue`])
/// back into a [`PublishQueueModel`]. Returns an error string on any malformed
/// input.
///
/// Public surface (re-exported via `nmp_core::typed_projections`): the
/// reachable decode entry point for the `publish_queue` sidecar key.
pub fn decode_publish_queue(bytes: &[u8]) -> Result<PublishQueueModel, String> {
    if bytes.len() < 8 || !fb::publish_queue_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KPBQ file identifier".to_string());
    }
    let root = fb::root_as_publish_queue_snapshot(bytes)
        .map_err(|e| format!("not a valid PublishQueueSnapshot buffer: {e}"))?;

    let mut entries = Vec::new();
    if let Some(fb_entries) = root.entries() {
        entries.reserve(fb_entries.len());
        for entry in fb_entries.iter() {
            let mut relay_outcomes = Vec::new();
            if let Some(fb_outcomes) = entry.relay_outcomes() {
                relay_outcomes.reserve(fb_outcomes.len());
                for outcome in fb_outcomes.iter() {
                    relay_outcomes.push(RelayAckOutcomeRow {
                        relay_url: outcome.relay_url().unwrap_or_default().to_string(),
                        status: outcome.status().unwrap_or_default().to_string(),
                        message: outcome.message().unwrap_or_default().to_string(),
                        relay_reason: outcome.relay_reason().unwrap_or_default().to_string(),
                    });
                }
            }
            entries.push(PublishQueueEntryRow {
                event_id: entry.event_id().unwrap_or_default().to_string(),
                kind: entry.kind(),
                title: entry.title().unwrap_or_default().to_string(),
                target_relays: entry.target_relays(),
                status: entry.status().unwrap_or_default().to_string(),
                can_retry: entry.can_retry(),
                relay_outcomes,
            });
        }
    }

    Ok(PublishQueueModel { entries })
}

#[cfg(test)]
#[path = "publish_queue_fb_tests.rs"]
mod tests;
