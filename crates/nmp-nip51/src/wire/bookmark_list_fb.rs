//! Typed FlatBuffers wire codec for the `"nmp.nip51.bookmarks"` projection.
//!
//! The authoritative read model is [`crate::BookmarkListSnapshot`], owned by
//! [`crate::BookmarkListProjection`]. This module encodes that same model as a
//! typed snapshot sidecar so hosts can render bookmark state without duplicating
//! NIP-51 list parsing or read-modify-write state.

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
#[path = "generated/bookmark_list_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::nip_51 as fb;

use crate::{BookmarkItem, BookmarkListSnapshot};

include!("bookmark_list_producer_consts.generated.rs");

fn item_wire_fields(item: &BookmarkItem) -> (fb::BookmarkListItemKind, &str, Option<&str>) {
    match item {
        BookmarkItem::Event { event_id, relay } => (
            fb::BookmarkListItemKind::Event,
            event_id.as_str(),
            relay.as_deref(),
        ),
        BookmarkItem::Address { coordinate, relay } => (
            fb::BookmarkListItemKind::Address,
            coordinate.as_str(),
            relay.as_deref(),
        ),
        BookmarkItem::Url { url } => (fb::BookmarkListItemKind::Url, url.as_str(), None),
        BookmarkItem::Hashtag { hashtag } => {
            (fb::BookmarkListItemKind::Hashtag, hashtag.as_str(), None)
        }
    }
}

fn item_from_wire(
    kind: fb::BookmarkListItemKind,
    value: &str,
    relay: Option<&str>,
) -> Result<BookmarkItem, String> {
    let value = value.to_string();
    let relay = relay.map(str::to_string);
    match kind {
        fb::BookmarkListItemKind::Event => Ok(BookmarkItem::Event {
            event_id: value,
            relay,
        }),
        fb::BookmarkListItemKind::Address => Ok(BookmarkItem::Address {
            coordinate: value,
            relay,
        }),
        fb::BookmarkListItemKind::Url => Ok(BookmarkItem::Url { url: value }),
        fb::BookmarkListItemKind::Hashtag => Ok(BookmarkItem::Hashtag { hashtag: value }),
        other => Err(format!(
            "unknown BookmarkListItemKind discriminant {}",
            other.0
        )),
    }
}

/// Encode a [`BookmarkListSnapshot`] to typed FlatBuffers bytes.
#[must_use]
pub fn encode_bookmark_list(snapshot: &BookmarkListSnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let mut offsets: Vec<WIPOffset<fb::BookmarkListItem<'_>>> =
        Vec::with_capacity(snapshot.items.len());
    for item in &snapshot.items {
        let (kind, value, relay) = item_wire_fields(item);
        let value = fbb.create_string(value);
        let relay = relay.map(|hint| fbb.create_string(hint));
        offsets.push(fb::BookmarkListItem::create(
            &mut fbb,
            &fb::BookmarkListItemArgs {
                kind,
                value: Some(value),
                relay,
            },
        ));
    }
    let items = fbb.create_vector(&offsets);
    let root = fb::BookmarkListSnapshot::create(
        &mut fbb,
        &fb::BookmarkListSnapshotArgs { items: Some(items) },
    );
    fb::finish_bookmark_list_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

/// Decode typed FlatBuffers bytes back into a [`BookmarkListSnapshot`].
pub fn decode_bookmark_list(bytes: &[u8]) -> Result<BookmarkListSnapshot, String> {
    if bytes.len() < 8 || !fb::bookmark_list_snapshot_buffer_has_identifier(bytes) {
        return Err("missing N51L file identifier".to_string());
    }
    let root = fb::root_as_bookmark_list_snapshot(bytes)
        .map_err(|e| format!("not a valid BookmarkListSnapshot buffer: {e}"))?;
    let mut items = Vec::new();
    if let Some(fb_items) = root.items() {
        items.reserve(fb_items.len());
        for item in fb_items.iter() {
            items.push(item_from_wire(item.kind(), item.value(), item.relay())?);
        }
    }
    Ok(BookmarkListSnapshot {
        items,
        metadata: Default::default(),
    })
}

#[cfg(test)]
#[path = "bookmark_list_fb_tests.rs"]
mod tests;
