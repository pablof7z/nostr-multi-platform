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
    pub attempt: u32,
    pub message: String,
    pub relay_reason: String,
}

/// One in-flight publish — a field-for-field mirror of the SERIALISED
/// [`PublishOutboxItem`](crate::kernel::PublishOutboxItem).
///
/// V-115 / ADR-0032: `created_at_display` and `target_summary` fully
/// removed from the schema. `created_at` (raw Unix-seconds u64) carries
/// the timestamp; shells format with their own locale.
/// ADR-0032 / doctrine §4.4: `title`, `preview`, `system_image`,
/// `status_label` pre-formatted strings removed; `content` (raw event
/// content) added so shells can render their own presentation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PublishOutboxItemRow {
    pub handle: String,
    pub event_id: String,
    pub kind: u32,
    /// Raw verbatim event content. Shells format for display (truncation,
    /// encrypted-content placeholder, kind-specific preview, etc.).
    pub content: String,
    /// Raw Unix-seconds creation timestamp (ADR-0032). Replaces
    /// `created_at_display`; shells format with their own locale + TZ.
    pub created_at: u64,
    pub status: String,
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
                    let message = fbb.create_string(&relay.message);
                    let relay_reason = fbb.create_string(&relay.relay_reason);
                    fb::PublishOutboxRelay::create(
                        &mut fbb,
                        &fb::PublishOutboxRelayArgs {
                            relay_url: Some(relay_url),
                            status: Some(status),
                            attempt: relay.attempt,
                            message: Some(message),
                            relay_reason: Some(relay_reason),
                            ..Default::default()
                        },
                    )
                })
                .collect();
            let relays = fbb.create_vector(&relay_offsets);

            let handle = fbb.create_string(&item.handle);
            let event_id = fbb.create_string(&item.event_id);
            let content = fbb.create_string(&item.content);
            let status = fbb.create_string(&item.status);
            // V-115 / ADR-0032: `created_at_display` and `target_summary`
            // removed from schema (fully deleted, not tombstoned). Pass raw
            // `created_at` (uint64) so shells format with their own locale.
            // ADR-0032 / doctrine §4.4: `title`, `preview`, `system_image`,
            // `status_label` removed; `content` (raw event content) added.
            fb::PublishOutboxItem::create(
                &mut fbb,
                &fb::PublishOutboxItemArgs {
                    handle: Some(handle),
                    event_id: Some(event_id),
                    kind: item.kind,
                    content: Some(content),
                    status: Some(status),
                    can_retry: item.can_retry,
                    target_relays: item.target_relays,
                    relays: Some(relays),
                    created_at: item.created_at,
                    ..Default::default()
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
                        attempt: relay.attempt(),
                        message: relay.message().unwrap_or_default().to_string(),
                        relay_reason: relay.relay_reason().unwrap_or_default().to_string(),
                    });
                }
            }
            // V-115 / ADR-0032: `created_at_display` and `target_summary`
            // removed from schema; decode `created_at` (raw uint64).
            // ADR-0032 / doctrine §4.4: `title`, `preview`, `system_image`,
            // `status_label` removed; `content` added.
            items.push(PublishOutboxItemRow {
                handle: item.handle().unwrap_or_default().to_string(),
                event_id: item.event_id().unwrap_or_default().to_string(),
                kind: item.kind(),
                content: item.content().unwrap_or_default().to_string(),
                created_at: item.created_at(),
                status: item.status().unwrap_or_default().to_string(),
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
