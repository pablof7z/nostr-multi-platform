//! Typed FlatBuffers wire codec for the `nmp.marmot.messages` projection
//! (`MarmotProjection::messages_all_groups_json` — `{ group_id_hex ->
//! [MarmotMessageRow] }`).
//!
//! The authoritative shape is the serde JSON map emitted by
//! `MarmotProjection::messages_all_groups_json`. This module adds a **typed
//! FlatBuffers** encoding of the same data carried in the `typed_projections`
//! sidecar (ADR-0072). The serde shape stays authoritative.
//!
//! Map flattening: FlatBuffers has no map type, so the per-group map is
//! flattened to a vector of `MarmotGroupMessages` **sorted by `group_id_hex`
//! ascending** for a deterministic wire (the nip29/zaps precedent). Each group's
//! `messages` preserve the order `ops::group_messages` produced. The encoder
//! takes a `&[(String, Vec<MarmotMessageRow>)]` so the sibling
//! `MarmotProjection::messages_all_groups` method can build the same data the
//! JSON projection emits — without touching the authoritative JSON path.
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input.

// The generated FlatBuffers bindings are intrinsically `unsafe`. This `allow`
// block scopes the relaxation to the single generated module — no hand-written
// code in this file uses `unsafe`.
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
#[path = "generated/marmot_messages_generated.rs"]
pub mod generated;

use generated::nmp::marmot as fb;

use crate::projection::payload::MarmotMessageRow;
use nmp_core::TypedProjectionData;

/// Host-declared projection key this typed payload is emitted under.
pub const PROJECTION_KEY: &str = "nmp.marmot.messages";
/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.marmot.messages";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NMMG";
/// Wire schema version. Bump on any breaking change to `marmot_messages.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

/// One flattened map entry: a group id and its newest-N message rows.
pub type GroupMessages = (String, Vec<MarmotMessageRow>);

// --- typed-projection envelope -------------------------------------------

/// Build the [`TypedProjectionData`] sidecar entry for the flattened all-groups
/// message map. The caller (the sibling `messages_all_groups` projection
/// method) supplies the per-group entries; this function sorts them by
/// `group_id_hex` and encodes.
#[must_use]
pub fn typed_projection(groups: &[GroupMessages]) -> TypedProjectionData {
    TypedProjectionData {
        key: PROJECTION_KEY.to_string(),
        schema_id: SCHEMA_ID.to_string(),
        schema_version: SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(FILE_IDENTIFIER).into_owned(),
        payload: encode_marmot_messages(groups),
        ..Default::default()
    }
}

// --- encode ---------------------------------------------------------------

/// Encode the flattened all-groups message map to typed FlatBuffers bytes (with
/// the `NMMG` file identifier). Groups are sorted by `group_id_hex` ascending
/// for a deterministic wire regardless of the input order.
#[must_use]
pub fn encode_marmot_messages(groups: &[GroupMessages]) -> Vec<u8> {
    let mut sorted: Vec<&GroupMessages> = groups.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let group_offsets: Vec<_> = sorted
        .iter()
        .map(|(gid, rows)| encode_group_messages(&mut fbb, gid, rows))
        .collect();
    let groups = fbb.create_vector(&group_offsets);

    let root = fb::MarmotMessages::create(
        &mut fbb,
        &fb::MarmotMessagesArgs {
            schema_version: SCHEMA_VERSION,
            groups: Some(groups),
        },
    );
    fb::finish_marmot_messages_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

type Off<'a, T> = flatbuffers::WIPOffset<T>;

fn encode_group_messages<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    group_id_hex: &str,
    rows: &[MarmotMessageRow],
) -> Off<'a, fb::MarmotGroupMessages<'a>> {
    let gid = fbb.create_string(group_id_hex);
    let row_offsets: Vec<_> = rows.iter().map(|r| encode_message_row(fbb, r)).collect();
    let messages = fbb.create_vector(&row_offsets);
    fb::MarmotGroupMessages::create(
        fbb,
        &fb::MarmotGroupMessagesArgs {
            group_id_hex: Some(gid),
            messages: Some(messages),
        },
    )
}

fn encode_message_row<'a>(
    fbb: &mut flatbuffers::FlatBufferBuilder<'a>,
    r: &MarmotMessageRow,
) -> Off<'a, fb::MarmotMessageRow<'a>> {
    let id = fbb.create_string(&r.id);
    let sender_pubkey_hex = fbb.create_string(&r.sender_pubkey_hex);
    let content = fbb.create_string(&r.content);
    fb::MarmotMessageRow::create(
        fbb,
        &fb::MarmotMessageRowArgs {
            id: Some(id),
            sender_pubkey_hex: Some(sender_pubkey_hex),
            content: Some(content),
            created_at: r.created_at,
            has_epoch: r.epoch.is_some(),
            epoch: r.epoch.unwrap_or(0),
        },
    )
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_marmot_messages`])
/// back into the flattened, sorted all-groups message vector. Returns an error
/// string on any malformed input.
pub fn decode_marmot_messages(bytes: &[u8]) -> Result<Vec<GroupMessages>, String> {
    if bytes.len() < 8 || !fb::marmot_messages_buffer_has_identifier(bytes) {
        return Err("missing NMMG file identifier".to_string());
    }
    let root = fb::root_as_marmot_messages(bytes)
        .map_err(|e| format!("not a valid MarmotMessages buffer: {e}"))?;

    let groups = root
        .groups()
        .map(|v| {
            v.iter()
                .map(|g| {
                    let gid = g.group_id_hex().unwrap_or_default().to_string();
                    let rows = g
                        .messages()
                        .map(|m| m.iter().map(decode_message_row).collect())
                        .unwrap_or_default();
                    (gid, rows)
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(groups)
}

fn decode_message_row(r: fb::MarmotMessageRow<'_>) -> MarmotMessageRow {
    MarmotMessageRow {
        id: r.id().unwrap_or_default().to_string(),
        sender_pubkey_hex: r.sender_pubkey_hex().unwrap_or_default().to_string(),
        content: r.content().unwrap_or_default().to_string(),
        created_at: r.created_at(),
        epoch: r.has_epoch().then_some(r.epoch()),
    }
}

#[cfg(test)]
#[path = "messages_fb_tests.rs"]
mod tests;
