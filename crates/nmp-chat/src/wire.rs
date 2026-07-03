//! Typed FlatBuffers codec for [`crate::ChatPresenceSnapshot`].

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
#[path = "wire/generated/chat_presence_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use generated::nmp::chat as fb;

use crate::presence::{ChatPresenceSnapshot, ChatPresenceTyping, ReadMarker};

include!("wire/chat_presence_producer_consts.generated.rs");

#[must_use]
pub fn encode_chat_presence_snapshot(snapshot: &ChatPresenceSnapshot) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let host_relay_url = fbb.create_string(&snapshot.host_relay_url);
    let group_id = fbb.create_string(&snapshot.group_id);
    let active_pubkey = fbb.create_string(&snapshot.active_pubkey);
    let read_marker = snapshot
        .read_marker
        .as_ref()
        .map(|marker| encode_read_marker(&mut fbb, marker));
    let typing_offsets: Vec<WIPOffset<fb::TypingParticipant<'_>>> = snapshot
        .typing
        .iter()
        .map(|typing| encode_typing(&mut fbb, typing))
        .collect();
    let typing = fbb.create_vector(&typing_offsets);

    let root = fb::ChatPresenceSnapshot::create(
        &mut fbb,
        &fb::ChatPresenceSnapshotArgs {
            schema_version: CHAT_PRESENCE_SCHEMA_VERSION,
            host_relay_url: Some(host_relay_url),
            group_id: Some(group_id),
            active_pubkey: Some(active_pubkey),
            read_marker,
            unread_count: snapshot.unread_count,
            typing: Some(typing),
        },
    );
    fb::finish_chat_presence_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_read_marker<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    marker: &ReadMarker,
) -> WIPOffset<fb::ReadMarker<'a>> {
    let event_id = fbb.create_string(&marker.event_id);
    fb::ReadMarker::create(
        fbb,
        &fb::ReadMarkerArgs {
            event_id: Some(event_id),
            created_at: marker.created_at,
        },
    )
}

fn encode_typing<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    typing: &ChatPresenceTyping,
) -> WIPOffset<fb::TypingParticipant<'a>> {
    let pubkey = fbb.create_string(&typing.pubkey);
    fb::TypingParticipant::create(
        fbb,
        &fb::TypingParticipantArgs {
            pubkey: Some(pubkey),
            updated_at_ms: typing.updated_at_ms,
            expires_at_ms: typing.expires_at_ms,
        },
    )
}

pub fn decode_chat_presence_snapshot(bytes: &[u8]) -> Result<ChatPresenceSnapshot, String> {
    if bytes.len() < 8 || !fb::chat_presence_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NCHP file identifier".to_string());
    }
    let root = fb::root_as_chat_presence_snapshot(bytes)
        .map_err(|e| format!("not a valid ChatPresenceSnapshot buffer: {e}"))?;

    let read_marker = match root.read_marker() {
        Some(marker) => Some(ReadMarker {
            event_id: str_field(marker.event_id(), "ReadMarker.event_id")?,
            created_at: marker.created_at(),
        }),
        None => None,
    };

    let mut typing = Vec::new();
    if let Some(fb_typing) = root.typing() {
        typing.reserve(fb_typing.len());
        for row in fb_typing.iter() {
            typing.push(ChatPresenceTyping {
                pubkey: str_field(row.pubkey(), "TypingParticipant.pubkey")?,
                updated_at_ms: row.updated_at_ms(),
                expires_at_ms: row.expires_at_ms(),
            });
        }
    }

    Ok(ChatPresenceSnapshot {
        host_relay_url: str_field(root.host_relay_url(), "ChatPresenceSnapshot.host_relay_url")?,
        group_id: str_field(root.group_id(), "ChatPresenceSnapshot.group_id")?,
        active_pubkey: str_field(root.active_pubkey(), "ChatPresenceSnapshot.active_pubkey")?,
        read_marker,
        unread_count: root.unread_count(),
        typing,
    })
}

fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
