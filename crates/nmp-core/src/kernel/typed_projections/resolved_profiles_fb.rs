//! Typed FlatBuffers wire codec for the kernel-owned `"resolved_profiles"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"resolved_profiles"`: the serialisation of `resolved_profiles()` (a
//! `BTreeMap<String, ProfileCard>` — the pre-merged pubkey -> profile card map
//! every consumer reads). This module adds a **typed FlatBuffers** encoding of
//! the same shape, carried in the `typed_projections` sidecar (ADR-0037)
//! ALONGSIDE — never replacing — the generic `Value` projection.
//!
//! FlatBuffers has no map type, so [`ResolvedProfilesModel`] flattens the map to
//! a vector of `{key, value}` entries; the producer `BTreeMap` is already
//! key-sorted, preserved verbatim. It reuses [`ProfileCardModel`](super::ProfileCardModel)
//! (the shared card shape from the `profile` codec), encoding it into THIS
//! module's own generated `ProfileCard` table. The model is built from the same
//! `resolved_profiles()` output the JSON path serialises, in the same tick, so
//! the two wire forms cannot diverge.
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
#[path = "generated/resolved_profiles_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use super::ProfileCardModel;
use generated::nmp::kernel as fb;
// Shared `ProfileCard` row type (`include`d from `profile_card.fbs`): lives in
// the crate-root `profile_card_generated` wrapper, not this module's `generated`.
use crate::profile_card_generated as pc;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const RESOLVED_PROFILES_SCHEMA_ID: &str = "resolved_profiles";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const RESOLVED_PROFILES_FILE_IDENTIFIER: &[u8; 4] = b"KRPR";
/// Wire schema version. Bump on any breaking change to `resolved_profiles.fbs`.
pub const RESOLVED_PROFILES_SCHEMA_VERSION: u32 = 1;

/// The `"resolved_profiles"` read model — the `pubkey -> ProfileCard` map
/// flattened to a key-sorted vector of `(key, value)` entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedProfilesModel {
    /// `(key, value)` entries, sorted by `key` (BTreeMap order, matches JSON).
    pub entries: Vec<(String, ProfileCardModel)>,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`ProfileCardModel`] into the SHARED generated `ProfileCard` table
/// (`include`d from `profile_card.fbs`). Mirrors
/// `profile_fb::create_profile_card`; both now reference the same `pc::ProfileCard`.
fn create_profile_card<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    card: &ProfileCardModel,
) -> WIPOffset<pc::ProfileCard<'a>> {
    let pubkey = fbb.create_string(&card.pubkey);
    // ADR-0032 / V-115: `npub` deprecated in schema; no `npub` arg in
    // `ProfileCardArgs` (flatc omits args for deprecated fields).
    let display_name = card.display_name.as_ref().map(|v| fbb.create_string(v));
    let picture_url = card.picture_url.as_ref().map(|v| fbb.create_string(v));
    let nip05 = fbb.create_string(&card.nip05);
    let about = fbb.create_string(&card.about);
    let lnurl = card.lnurl.as_ref().map(|v| fbb.create_string(v));
    pc::ProfileCard::create(
        fbb,
        &pc::ProfileCardArgs {
            pubkey: Some(pubkey),
            has_display_name: card.display_name.is_some(),
            display_name,
            has_picture_url: card.picture_url.is_some(),
            picture_url,
            nip05: Some(nip05),
            about: Some(about),
            has_lnurl: card.lnurl.is_some(),
            lnurl,
        },
    )
}

/// Encode a [`ResolvedProfilesModel`] to typed FlatBuffers bytes (with the
/// `KRPR` file identifier).
#[must_use]
pub(crate) fn encode_resolved_profiles(model: &ResolvedProfilesModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let entry_offsets: Vec<WIPOffset<fb::ResolvedProfileEntry<'_>>> = model
        .entries
        .iter()
        .map(|(key, card)| {
            let key = fbb.create_string(key);
            let value = create_profile_card(&mut fbb, card);
            fb::ResolvedProfileEntry::create(
                &mut fbb,
                &fb::ResolvedProfileEntryArgs {
                    key: Some(key),
                    value: Some(value),
                },
            )
        })
        .collect();
    let entries = fbb.create_vector(&entry_offsets);
    let root = fb::ResolvedProfilesSnapshot::create(
        &mut fbb,
        &fb::ResolvedProfilesSnapshotArgs {
            entries: Some(entries),
        },
    );
    fb::finish_resolved_profiles_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode the SHARED generated `ProfileCard` table (`include`d from
/// `profile_card.fbs`) into a [`ProfileCardModel`].
fn profile_card_from_fb(card: pc::ProfileCard<'_>) -> ProfileCardModel {
    ProfileCardModel {
        pubkey: card.pubkey().unwrap_or_default().to_string(),
        // ADR-0032 / V-115: `npub` deprecated; no accessor generated. Empty.
        npub: String::new(),
        display_name: card
            .has_display_name()
            .then(|| card.display_name().unwrap_or_default().to_string()),
        picture_url: card
            .has_picture_url()
            .then(|| card.picture_url().unwrap_or_default().to_string()),
        nip05: card.nip05().unwrap_or_default().to_string(),
        about: card.about().unwrap_or_default().to_string(),
        lnurl: card
            .has_lnurl()
            .then(|| card.lnurl().unwrap_or_default().to_string()),
    }
}

/// Decode typed FlatBuffers bytes (as produced by [`encode_resolved_profiles`])
/// back into a [`ResolvedProfilesModel`]. Returns an error string on any
/// malformed input.
pub fn decode_resolved_profiles(bytes: &[u8]) -> Result<ResolvedProfilesModel, String> {
    if bytes.len() < 8 || !fb::resolved_profiles_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KRPR file identifier".to_string());
    }
    let root = fb::root_as_resolved_profiles_snapshot(bytes)
        .map_err(|e| format!("not a valid ResolvedProfilesSnapshot buffer: {e}"))?;

    let mut entries = Vec::new();
    if let Some(fb_entries) = root.entries() {
        entries.reserve(fb_entries.len());
        for entry in fb_entries.iter() {
            let key = entry.key().unwrap_or_default().to_string();
            let value = entry.value().map(profile_card_from_fb).unwrap_or_default();
            entries.push((key, value));
        }
    }
    Ok(ResolvedProfilesModel { entries })
}

#[cfg(test)]
#[path = "resolved_profiles_fb_tests.rs"]
mod tests;
