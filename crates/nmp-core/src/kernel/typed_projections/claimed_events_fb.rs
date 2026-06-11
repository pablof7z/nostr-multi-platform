//! Typed FlatBuffers wire codec for the kernel-owned `"claimed_events"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"claimed_events"`: the serialisation of `claimed_events()` (a
//! `BTreeMap<String, ClaimedEventDto>` — primary_id -> resolved+enriched event).
//! This module adds a **typed FlatBuffers** encoding of the same shape, carried
//! in the `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing —
//! the generic `Value` projection.
//!
//! FlatBuffers has no map type, so [`ClaimedEventsModel`] flattens the map to a
//! vector of `{key, value}` entries; the producer `BTreeMap` is already
//! key-sorted, preserved verbatim. The raw `tags: Vec<Vec<String>>` is modelled
//! as `[TagRow]` (FlatBuffers cannot nest vectors directly). The model is built
//! from the same `claimed_events()` output the JSON path serialises, in the same
//! tick, so the two wire forms cannot diverge.
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
#[path = "generated/claimed_events_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const CLAIMED_EVENTS_SCHEMA_ID: &str = "claimed_events";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const CLAIMED_EVENTS_FILE_IDENTIFIER: &[u8; 4] = b"KCEV";
/// Wire schema version. Bump on any breaking change to `claimed_events.fbs`.
pub const CLAIMED_EVENTS_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of the SERIALISED `ClaimedEventDto` — one resolved
/// event value in the `claimed_events` map.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClaimedEventRow {
    pub primary_id: String,
    pub id: String,
    pub author_pubkey: String,
    pub author_display_name: Option<String>,
    pub author_picture_url: Option<String>,
    pub kind: u32,
    pub created_at: u64,
    /// Raw event tags — an array of tag rows, each a list of strings.
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

/// The `"claimed_events"` read model — the `primary_id -> ClaimedEventDto` map
/// flattened to a key-sorted vector of `(key, value)` entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClaimedEventsModel {
    /// `(key, value)` entries, sorted by `key` (BTreeMap order, matches JSON).
    pub entries: Vec<(String, ClaimedEventRow)>,
}

// --- encode ---------------------------------------------------------------

/// Encode one [`ClaimedEventRow`] into this module's generated `ClaimedEvent`
/// table.
fn create_claimed_event<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &ClaimedEventRow,
) -> WIPOffset<fb::ClaimedEvent<'a>> {
    let primary_id = fbb.create_string(&row.primary_id);
    let id = fbb.create_string(&row.id);
    let author_pubkey = fbb.create_string(&row.author_pubkey);
    let author_display_name = row
        .author_display_name
        .as_ref()
        .map(|v| fbb.create_string(v));
    let author_picture_url = row
        .author_picture_url
        .as_ref()
        .map(|v| fbb.create_string(v));
    let content = fbb.create_string(&row.content);

    // `tags: Vec<Vec<String>>` → `[TagRow]`, each `TagRow` wrapping one inner
    // `[string]`. Inner string offsets must be created before the TagRow.
    let tag_offsets: Vec<WIPOffset<fb::TagRow<'_>>> = row
        .tags
        .iter()
        .map(|tag| {
            let value_offsets: Vec<WIPOffset<&str>> =
                tag.iter().map(|s| fbb.create_string(s)).collect();
            let values = fbb.create_vector(&value_offsets);
            fb::TagRow::create(
                fbb,
                &fb::TagRowArgs {
                    values: Some(values),
                },
            )
        })
        .collect();
    let tags = fbb.create_vector(&tag_offsets);

    fb::ClaimedEvent::create(
        fbb,
        &fb::ClaimedEventArgs {
            primary_id: Some(primary_id),
            id: Some(id),
            author_pubkey: Some(author_pubkey),
            has_author_display_name: row.author_display_name.is_some(),
            author_display_name,
            has_author_picture_url: row.author_picture_url.is_some(),
            author_picture_url,
            kind: row.kind,
            created_at: row.created_at,
            tags: Some(tags),
            content: Some(content),
        },
    )
}

/// Encode a [`ClaimedEventsModel`] to typed FlatBuffers bytes (with the `KCEV`
/// file identifier).
#[must_use]
pub(crate) fn encode_claimed_events(model: &ClaimedEventsModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let entry_offsets: Vec<WIPOffset<fb::ClaimedEventEntry<'_>>> = model
        .entries
        .iter()
        .map(|(key, row)| {
            let key = fbb.create_string(key);
            let value = create_claimed_event(&mut fbb, row);
            fb::ClaimedEventEntry::create(
                &mut fbb,
                &fb::ClaimedEventEntryArgs {
                    key: Some(key),
                    value: Some(value),
                },
            )
        })
        .collect();
    let entries = fbb.create_vector(&entry_offsets);
    let root = fb::ClaimedEventsSnapshot::create(
        &mut fbb,
        &fb::ClaimedEventsSnapshotArgs {
            entries: Some(entries),
        },
    );
    fb::finish_claimed_events_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode this module's generated `ClaimedEvent` table into a
/// [`ClaimedEventRow`].
fn claimed_event_from_fb(row: fb::ClaimedEvent<'_>) -> ClaimedEventRow {
    let tags = row
        .tags()
        .map(|fb_tags| {
            fb_tags
                .iter()
                .map(|tag| {
                    tag.values()
                        .map(|vs| vs.iter().map(|s| s.to_string()).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    ClaimedEventRow {
        primary_id: row.primary_id().unwrap_or_default().to_string(),
        id: row.id().unwrap_or_default().to_string(),
        author_pubkey: row.author_pubkey().unwrap_or_default().to_string(),
        author_display_name: row
            .has_author_display_name()
            .then(|| row.author_display_name().unwrap_or_default().to_string()),
        author_picture_url: row
            .has_author_picture_url()
            .then(|| row.author_picture_url().unwrap_or_default().to_string()),
        kind: row.kind(),
        created_at: row.created_at(),
        tags,
        content: row.content().unwrap_or_default().to_string(),
    }
}

/// Decode typed FlatBuffers bytes (as produced by [`encode_claimed_events`])
/// back into a [`ClaimedEventsModel`]. Returns an error string on any malformed
/// input.
pub fn decode_claimed_events(bytes: &[u8]) -> Result<ClaimedEventsModel, String> {
    if bytes.len() < 8 || !fb::claimed_events_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KCEV file identifier".to_string());
    }
    let root = fb::root_as_claimed_events_snapshot(bytes)
        .map_err(|e| format!("not a valid ClaimedEventsSnapshot buffer: {e}"))?;

    let mut entries = Vec::new();
    if let Some(fb_entries) = root.entries() {
        entries.reserve(fb_entries.len());
        for entry in fb_entries.iter() {
            let key = entry.key().unwrap_or_default().to_string();
            let value = entry.value().map(claimed_event_from_fb).unwrap_or_default();
            entries.push((key, value));
        }
    }
    Ok(ClaimedEventsModel { entries })
}

#[cfg(test)]
#[path = "claimed_events_fb_tests.rs"]
mod tests;
