//! Typed FlatBuffers payload codec for the nip51 bookmark ACTIONS
//! (ADR-0071 / S9 #1747): `nmp.nip51.add_bookmark` / `nmp.nip51.remove_bookmark`
//! ([`BookmarkUpdateInput`]). Both namespaces carry the SAME input shape, so they
//! share this one codec.
//!
//! This is the WRITE-direction typed payload carried as the OPAQUE
//! `DispatchEnvelope.payload`. The registry adapter decodes it through
//! [`ActionPayload::decode`] here — the single typed-decode site — running the
//! fail-closed `schema_version` gate BEFORE `start()`. Distinct from
//! `mute_list_fb.rs`, which is the READ-direction snapshot sidecar.
//!
//! The nested [`BookmarkItem`] sum is modelled as a tagged table: a
//! `BookmarkItemKind` discriminator plus optional payload fields. The decode
//! reconstructs the exact `BookmarkItem` variant from the discriminator and
//! preserves the optional `relay` presence faithfully (`Some("")` never
//! collapses to `None`).
//!
//! Honours D6: decode returns a data-shaped [`ActionPayloadDecodeError`] on any
//! malformed input; no panics on the decode path.

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
#[path = "generated/bookmark_update_generated.rs"]
pub mod generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use generated::nmp::nip_51 as fb;

use crate::bookmarks::{BookmarkItem, BookmarkUpdateInput};

/// Wire schema version for the nip51 bookmark-update payload. Bump on any
/// breaking change to `bookmark_update.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

/// Project a [`BookmarkItem`] into the tagged-table wire fields
/// `(kind, value, relay)`.
fn item_wire_fields(item: &BookmarkItem) -> (fb::BookmarkItemKind, &str, Option<&str>) {
    match item {
        BookmarkItem::Event { event_id, relay } => (
            fb::BookmarkItemKind::Event,
            event_id.as_str(),
            relay.as_deref(),
        ),
        BookmarkItem::Address { coordinate, relay } => (
            fb::BookmarkItemKind::Address,
            coordinate.as_str(),
            relay.as_deref(),
        ),
        BookmarkItem::Url { url } => (fb::BookmarkItemKind::Url, url.as_str(), None),
        BookmarkItem::Hashtag { hashtag } => {
            (fb::BookmarkItemKind::Hashtag, hashtag.as_str(), None)
        }
    }
}

/// Reconstruct a [`BookmarkItem`] from the tagged-table discriminator + fields.
/// Preserves the `relay` presence faithfully (no empty -> None collapse).
fn item_from_wire(
    kind: fb::BookmarkItemKind,
    value: &str,
    relay: Option<&str>,
) -> Result<BookmarkItem, ActionPayloadDecodeError> {
    let value = value.to_string();
    let relay = relay.map(str::to_string);
    match kind {
        fb::BookmarkItemKind::Event => Ok(BookmarkItem::Event {
            event_id: value,
            relay,
        }),
        fb::BookmarkItemKind::Address => Ok(BookmarkItem::Address {
            coordinate: value,
            relay,
        }),
        fb::BookmarkItemKind::Url => Ok(BookmarkItem::Url { url: value }),
        fb::BookmarkItemKind::Hashtag => Ok(BookmarkItem::Hashtag { hashtag: value }),
        other => Err(malformed(format!(
            "unknown BookmarkItemKind discriminant {}",
            other.0
        ))),
    }
}

impl ActionPayload for BookmarkUpdateInput {
    const SCHEMA_ID: &'static str = "nmp.nip51.bookmark_update";
    const SCHEMA_VERSION: u32 = SCHEMA_VERSION;

    fn encode(&self) -> Vec<u8> {
        let mut fbb = flatbuffers::FlatBufferBuilder::new();
        let (kind, value, relay) = item_wire_fields(&self.item);
        let value_off = fbb.create_string(value);
        let relay_off = relay.map(|r| fbb.create_string(r));
        let item = fb::BookmarkItem::create(
            &mut fbb,
            &fb::BookmarkItemArgs {
                kind,
                value: Some(value_off),
                relay: relay_off,
            },
        );
        let account_pubkey = fbb.create_string(&self.account_pubkey);
        let payload = fb::BookmarkUpdatePayload::create(
            &mut fbb,
            &fb::BookmarkUpdatePayloadArgs {
                schema_version: SCHEMA_VERSION,
                account_pubkey: Some(account_pubkey),
                item: Some(item),
            },
        );
        fb::finish_bookmark_update_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::bookmark_update_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N51B file identifier"));
        }
        let root = fb::root_as_bookmark_update_payload(bytes)
            .map_err(|e| malformed(format!("not a valid BookmarkUpdatePayload buffer: {e}")))?;
        // Gate FIRST.
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let item_fb = root.item();
        let item = item_from_wire(item_fb.kind(), item_fb.value(), item_fb.relay())?;
        Ok(BookmarkUpdateInput {
            account_pubkey: root.account_pubkey().to_string(),
            item,
        })
    }
}

#[cfg(test)]
#[path = "bookmark_update_fb_tests.rs"]
mod tests;
