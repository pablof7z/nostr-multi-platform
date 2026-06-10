//! Typed FlatBuffers wire codec for the kernel-owned `"mention_profiles"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"mention_profiles"`: the serialisation of `mention_profiles()` (a
//! `HashMap<String, MentionProfilePayload>` — pubkey -> raw kind:0 display
//! fields for every author surfaced in any open view). This module adds a
//! **typed FlatBuffers** encoding of the same shape, carried in the
//! `typed_projections` sidecar (ADR-0037) ALONGSIDE — never replacing — the
//! generic `Value` projection.
//!
//! FlatBuffers has no map type, so [`MentionProfilesModel`] flattens the map to
//! a vector of `{key, value}` entries SORTED BY KEY — the serde JSON map is
//! BTree-ordered, so sorting matches the JSON key order. The model is built from
//! the same `mention_profiles()` output the JSON path serialises, in the same
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
#[path = "generated/mention_profiles_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub(crate) const MENTION_PROFILES_SCHEMA_ID: &str = "mention_profiles";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub(crate) const MENTION_PROFILES_FILE_IDENTIFIER: &[u8; 4] = b"KMPR";
/// Wire schema version. Bump on any breaking change to `mention_profiles.fbs`.
pub(crate) const MENTION_PROFILES_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of the SERIALISED `MentionProfilePayload` — one
/// per-author value in the `mention_profiles` map.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MentionProfileRow {
    pub(crate) pubkey: String,
    pub(crate) display_name: Option<String>,
    pub(crate) picture_url: Option<String>,
}

/// The `"mention_profiles"` read model — the `pubkey -> MentionProfilePayload`
/// map flattened to a key-sorted vector of `(key, value)` entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct MentionProfilesModel {
    /// `(key, value)` entries, sorted by `key` (matches the BTree-ordered JSON).
    pub(crate) entries: Vec<(String, MentionProfileRow)>,
}

// --- encode ---------------------------------------------------------------

/// Encode one [`MentionProfileRow`] into this module's generated `MentionProfile`
/// table.
fn create_mention_profile<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &MentionProfileRow,
) -> WIPOffset<fb::MentionProfile<'a>> {
    let pubkey = fbb.create_string(&row.pubkey);
    let display_name = row.display_name.as_ref().map(|v| fbb.create_string(v));
    let picture_url = row.picture_url.as_ref().map(|v| fbb.create_string(v));
    fb::MentionProfile::create(
        fbb,
        &fb::MentionProfileArgs {
            pubkey: Some(pubkey),
            has_display_name: row.display_name.is_some(),
            display_name,
            has_picture_url: row.picture_url.is_some(),
            picture_url,
        },
    )
}

/// Encode a [`MentionProfilesModel`] to typed FlatBuffers bytes (with the `KMPR`
/// file identifier).
#[must_use]
pub(crate) fn encode_mention_profiles(model: &MentionProfilesModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let entry_offsets: Vec<WIPOffset<fb::MentionProfileEntry<'_>>> = model
        .entries
        .iter()
        .map(|(key, row)| {
            let key = fbb.create_string(key);
            let value = create_mention_profile(&mut fbb, row);
            fb::MentionProfileEntry::create(
                &mut fbb,
                &fb::MentionProfileEntryArgs {
                    key: Some(key),
                    value: Some(value),
                },
            )
        })
        .collect();
    let entries = fbb.create_vector(&entry_offsets);
    let root = fb::MentionProfilesSnapshot::create(
        &mut fbb,
        &fb::MentionProfilesSnapshotArgs {
            entries: Some(entries),
        },
    );
    fb::finish_mention_profiles_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_mention_profiles`])
/// back into a [`MentionProfilesModel`]. Returns an error string on any
/// malformed input.
#[cfg(test)]
pub(crate) fn decode_mention_profiles(bytes: &[u8]) -> Result<MentionProfilesModel, String> {
    if bytes.len() < 8 || !fb::mention_profiles_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KMPR file identifier".to_string());
    }
    let root = fb::root_as_mention_profiles_snapshot(bytes)
        .map_err(|e| format!("not a valid MentionProfilesSnapshot buffer: {e}"))?;

    let mut entries = Vec::new();
    if let Some(fb_entries) = root.entries() {
        entries.reserve(fb_entries.len());
        for entry in fb_entries.iter() {
            let key = entry.key().unwrap_or_default().to_string();
            let value = entry
                .value()
                .map(mention_profile_from_fb)
                .unwrap_or_default();
            entries.push((key, value));
        }
    }
    Ok(MentionProfilesModel { entries })
}

/// Decode this module's generated `MentionProfile` table into a
/// [`MentionProfileRow`].
#[cfg(test)]
fn mention_profile_from_fb(row: fb::MentionProfile<'_>) -> MentionProfileRow {
    MentionProfileRow {
        pubkey: row.pubkey().unwrap_or_default().to_string(),
        display_name: row
            .has_display_name()
            .then(|| row.display_name().unwrap_or_default().to_string()),
        picture_url: row
            .has_picture_url()
            .then(|| row.picture_url().unwrap_or_default().to_string()),
    }
}

#[cfg(test)]
#[path = "mention_profiles_fb_tests.rs"]
mod tests;
