//! Typed FlatBuffers payload codec for NIP-51 bookmark-set ACTIONS:
//! `nmp.nip51.add_bookmark_set_item` /
//! `nmp.nip51.remove_bookmark_set_item`.
//!
//! Both namespaces carry the same [`BookmarkSetUpdateInput`] shape. The registry
//! adapter decodes it through [`ActionPayload::decode`], gating
//! `schema_version` before `start()`.

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
#[path = "generated/bookmark_set_update_generated.rs"]
pub mod generated;

use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

use generated::nmp::nip_51 as fb;

use crate::bookmark_sets::{BookmarkSetKind, BookmarkSetUpdateInput};
use crate::bookmarks::BookmarkItem;

/// Wire schema version for the bookmark-set update payload.
pub const SCHEMA_VERSION: u32 = 1;

fn malformed(reason: impl Into<String>) -> ActionPayloadDecodeError {
    ActionPayloadDecodeError::Malformed {
        reason: reason.into(),
    }
}

fn set_kind_to_wire(kind: BookmarkSetKind) -> fb::BookmarkSetKindWire {
    match kind {
        BookmarkSetKind::BookmarkSet => fb::BookmarkSetKindWire::BookmarkSet,
        BookmarkSetKind::CurationSet => fb::BookmarkSetKindWire::CurationSet,
    }
}

fn set_kind_from_wire(
    kind: fb::BookmarkSetKindWire,
) -> Result<BookmarkSetKind, ActionPayloadDecodeError> {
    match kind {
        fb::BookmarkSetKindWire::BookmarkSet => Ok(BookmarkSetKind::BookmarkSet),
        fb::BookmarkSetKindWire::CurationSet => Ok(BookmarkSetKind::CurationSet),
        other => Err(malformed(format!(
            "unknown BookmarkSetKindWire discriminant {}",
            other.0
        ))),
    }
}

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

impl ActionPayload for BookmarkSetUpdateInput {
    const SCHEMA_ID: &'static str = "nmp.nip51.bookmark_set_update";
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
        let identifier = fbb.create_string(&self.identifier);
        let payload = fb::BookmarkSetUpdatePayload::create(
            &mut fbb,
            &fb::BookmarkSetUpdatePayloadArgs {
                schema_version: SCHEMA_VERSION,
                account_pubkey: Some(account_pubkey),
                set_kind: set_kind_to_wire(self.set_kind),
                identifier: Some(identifier),
                item: Some(item),
            },
        );
        fb::finish_bookmark_set_update_payload_buffer(&mut fbb, payload);
        fbb.finished_data().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError> {
        if bytes.len() < 8 || !fb::bookmark_set_update_payload_buffer_has_identifier(bytes) {
            return Err(malformed("missing N51S file identifier"));
        }
        let root = fb::root_as_bookmark_set_update_payload(bytes)
            .map_err(|e| malformed(format!("not a valid BookmarkSetUpdatePayload buffer: {e}")))?;
        let found = root.schema_version();
        if found != SCHEMA_VERSION {
            return Err(ActionPayloadDecodeError::SchemaVersionMismatch {
                found,
                expected: SCHEMA_VERSION,
            });
        }
        let item_fb = root.item();
        Ok(BookmarkSetUpdateInput {
            account_pubkey: root.account_pubkey().to_string(),
            set_kind: set_kind_from_wire(root.set_kind())?,
            identifier: root.identifier().to_string(),
            item: item_from_wire(item_fb.kind(), item_fb.value(), item_fb.relay())?,
        })
    }
}

#[cfg(test)]
#[path = "bookmark_set_update_fb_tests.rs"]
mod tests;
