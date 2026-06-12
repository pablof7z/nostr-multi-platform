//! Typed FlatBuffers wire codec for the kernel-owned `"publish_outbox"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"publish_outbox"`: the serialisation of `publish_outbox_items()`, a `Vec` of
//! [`PublishOutboxItem`](crate::kernel::PublishOutboxItem) (each owning a
//! `Vec<PublishOutboxRelay>`). This module adds a **typed FlatBuffers** encoding
//! of the same shape, carried in the `typed_projections` sidecar (ADR-0037)
//! ALONGSIDE — never replacing — the generic `Value` projection.
//!
//! [`PublishOutboxModel`] is built directly from the same item vector the JSON
//! path serialises (mapped inline in
//! [`Kernel::builtin_typed_projections`](crate::kernel::Kernel), where the
//! `pub(super)` DTO types are nameable), in the same tick, so the two wire forms
//! cannot structurally diverge.
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
#[path = "generated/publish_outbox_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const PUBLISH_OUTBOX_SCHEMA_ID: &str = "publish_outbox";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const PUBLISH_OUTBOX_FILE_IDENTIFIER: &[u8; 4] = b"KPBO";
/// Wire schema version. Bump on any breaking change to `publish_outbox.fbs`.
pub const PUBLISH_OUTBOX_SCHEMA_VERSION: u32 = 1;

/// One target relay of an in-flight publish — a field-for-field mirror of the
/// SERIALISED [`PublishOutboxRelay`](crate::kernel::PublishOutboxRelay).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PublishOutboxRelayRow {
    pub relay_url: String,
    pub status: String,
    pub status_label: String,
    pub attempt: u32,
    pub attempt_label: String,
    pub message: String,
    pub relay_reason: String,
}

/// One in-flight publish — a field-for-field mirror of the SERIALISED
/// [`PublishOutboxItem`](crate::kernel::PublishOutboxItem).
///
/// V-115 / ADR-0032: `created_at_display` and `target_summary` removed;
/// `created_at` (raw Unix-seconds u64) added. Shells format for display.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PublishOutboxItemRow {
    pub handle: String,
    pub event_id: String,
    pub kind: u32,
    pub title: String,
    pub preview: String,
    /// Raw Unix-seconds creation timestamp (ADR-0032). Replaces
    /// `created_at_display`; shells format with their own locale + TZ.
    pub created_at: u64,
    pub status: String,
    pub status_label: String,
    pub system_image: String,
    pub can_retry: bool,
    pub target_relays: u32,
    pub relays: Vec<PublishOutboxRelayRow>,
}

/// The `"publish_outbox"` read model — the ordered in-flight items. Built from
/// the same `PublishOutboxItem` vector the JSON projection serialises (mapped
/// inline in [`Kernel::builtin_typed_projections`](crate::kernel::Kernel)).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PublishOutboxModel {
    pub items: Vec<PublishOutboxItemRow>,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`PublishOutboxModel`] to typed FlatBuffers bytes (with the `KPBO`
/// file identifier). Item + nested-relay order is preserved verbatim.
#[must_use]
pub(crate) fn encode_publish_outbox(model: &PublishOutboxModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let item_offsets: Vec<WIPOffset<fb::PublishOutboxItem>> = model
        .items
        .iter()
        .map(|item| {
            let relay_offsets: Vec<WIPOffset<fb::PublishOutboxRelay>> = item
                .relays
                .iter()
                .map(|relay| {
                    let relay_url = fbb.create_string(&relay.relay_url);
                    let status = fbb.create_string(&relay.status);
                    let status_label = fbb.create_string(&relay.status_label);
                    let attempt_label = fbb.create_string(&relay.attempt_label);
                    let message = fbb.create_string(&relay.message);
                    let relay_reason = fbb.create_string(&relay.relay_reason);
                    fb::PublishOutboxRelay::create(
                        &mut fbb,
                        &fb::PublishOutboxRelayArgs {
                            relay_url: Some(relay_url),
                            status: Some(status),
                            status_label: Some(status_label),
                            attempt: relay.attempt,
                            attempt_label: Some(attempt_label),
                            message: Some(message),
                            relay_reason: Some(relay_reason),
                        },
                    )
                })
                .collect();
            let relays = fbb.create_vector(&relay_offsets);

            let handle = fbb.create_string(&item.handle);
            let event_id = fbb.create_string(&item.event_id);
            let title = fbb.create_string(&item.title);
            let preview = fbb.create_string(&item.preview);
            let status = fbb.create_string(&item.status);
            let status_label = fbb.create_string(&item.status_label);
            let system_image = fbb.create_string(&item.system_image);
            // ADR-0032 / V-115: `created_at_display` and `target_summary`
            // are deprecated in the schema; flatc removes them from
            // `PublishOutboxItemArgs`. Pass raw `created_at` in the new
            // uint64 field; the deprecated vtable slots stay 0/null.
            fb::PublishOutboxItem::create(
                &mut fbb,
                &fb::PublishOutboxItemArgs {
                    handle: Some(handle),
                    event_id: Some(event_id),
                    kind: item.kind,
                    title: Some(title),
                    preview: Some(preview),
                    status: Some(status),
                    status_label: Some(status_label),
                    system_image: Some(system_image),
                    can_retry: item.can_retry,
                    target_relays: item.target_relays,
                    relays: Some(relays),
                    created_at: item.created_at,
                },
            )
        })
        .collect();
    let items = fbb.create_vector(&item_offsets);

    let root = fb::PublishOutboxSnapshot::create(
        &mut fbb,
        &fb::PublishOutboxSnapshotArgs { items: Some(items) },
    );
    fb::finish_publish_outbox_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_publish_outbox`])
/// back into a [`PublishOutboxModel`]. Returns an error string on any malformed
/// input.
pub fn decode_publish_outbox(bytes: &[u8]) -> Result<PublishOutboxModel, String> {
    if bytes.len() < 8 || !fb::publish_outbox_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KPBO file identifier".to_string());
    }
    let root = fb::root_as_publish_outbox_snapshot(bytes)
        .map_err(|e| format!("not a valid PublishOutboxSnapshot buffer: {e}"))?;

    let mut items = Vec::new();
    if let Some(fb_items) = root.items() {
        items.reserve(fb_items.len());
        for item in fb_items.iter() {
            let mut relays = Vec::new();
            if let Some(fb_relays) = item.relays() {
                relays.reserve(fb_relays.len());
                for relay in fb_relays.iter() {
                    relays.push(PublishOutboxRelayRow {
                        relay_url: relay.relay_url().unwrap_or_default().to_string(),
                        status: relay.status().unwrap_or_default().to_string(),
                        status_label: relay.status_label().unwrap_or_default().to_string(),
                        attempt: relay.attempt(),
                        attempt_label: relay.attempt_label().unwrap_or_default().to_string(),
                        message: relay.message().unwrap_or_default().to_string(),
                        relay_reason: relay.relay_reason().unwrap_or_default().to_string(),
                    });
                }
            }
            // ADR-0032 / V-115: `created_at_display` and `target_summary`
            // deprecated; decode `created_at` (raw uint64) from the new slot.
            items.push(PublishOutboxItemRow {
                handle: item.handle().unwrap_or_default().to_string(),
                event_id: item.event_id().unwrap_or_default().to_string(),
                kind: item.kind(),
                title: item.title().unwrap_or_default().to_string(),
                preview: item.preview().unwrap_or_default().to_string(),
                created_at: item.created_at(),
                status: item.status().unwrap_or_default().to_string(),
                status_label: item.status_label().unwrap_or_default().to_string(),
                system_image: item.system_image().unwrap_or_default().to_string(),
                can_retry: item.can_retry(),
                target_relays: item.target_relays(),
                relays,
            });
        }
    }

    Ok(PublishOutboxModel { items })
}

#[cfg(test)]
#[path = "publish_outbox_fb_tests.rs"]
mod tests;
