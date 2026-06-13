//! Typed FlatBuffers wire codec for the kernel-owned `"claimed_profiles"`
//! projection (Tier-2 built-in).
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"claimed_profiles"`: the serialisation of `claimed_profiles()` (a
//! `BTreeMap<String, ProfileCard>` — pubkey -> profile card for every currently
//! claimed UI profile). This module adds a **typed FlatBuffers** encoding of the
//! same shape, carried in the `typed_projections` sidecar (ADR-0037) ALONGSIDE —
//! never replacing — the generic `Value` projection.
//!
//! FlatBuffers has no map type, so [`ClaimedProfilesModel`] flattens the map to
//! a vector of `{key, value}` entries; the producer `BTreeMap` is already
//! key-sorted, preserved verbatim. It reuses [`ProfileCardModel`](super::ProfileCardModel)
//! (the shared card shape from the `profile` codec), encoding it into THIS
//! module's own generated `ProfileCard` table. The model is built from the same
//! `claimed_profiles()` output the JSON path serialises, in the same tick, so
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
#[path = "generated/claimed_profiles_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use super::ProfileCardModel;
use generated::nmp::kernel as fb;
// Shared `ProfileCard` row type (`include`d from `profile_card.fbs`): lives in
// the crate-root `profile_card_generated` wrapper, not this module's `generated`.
use crate::profile_card_generated as pc;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const CLAIMED_PROFILES_SCHEMA_ID: &str = "claimed_profiles";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const CLAIMED_PROFILES_FILE_IDENTIFIER: &[u8; 4] = b"KCPR";
/// Wire schema version. Bump on any breaking change to `claimed_profiles.fbs`.
pub const CLAIMED_PROFILES_SCHEMA_VERSION: u32 = 1;

/// The `"claimed_profiles"` read model — the `pubkey -> ProfileCard` map
/// flattened to a key-sorted vector of `(key, value)` entries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ClaimedProfilesModel {
    /// `(key, value)` entries, sorted by `key` (BTreeMap order, matches JSON).
    pub entries: Vec<(String, ProfileCardModel)>,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`ProfileCardModel`] into the SHARED generated `ProfileCard` table
/// (`include`d from `profile_card.fbs`). Mirrors
/// `profile_fb::create_profile_card`; both now reference the same `pc::ProfileCard`.
pub(super) fn create_profile_card<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    card: &ProfileCardModel,
) -> WIPOffset<pc::ProfileCard<'a>> {
    let pubkey = fbb.create_string(&card.pubkey);
    // ADR-0032 / V-115: `npub` deprecated in schema; `ProfileCardArgs` no longer
    // has an `npub` arg (flatc omits args for deprecated fields).
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

/// Encode a [`ClaimedProfilesModel`] to typed FlatBuffers bytes (with the `KCPR`
/// file identifier).
#[must_use]
pub(crate) fn encode_claimed_profiles(model: &ClaimedProfilesModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let entry_offsets: Vec<WIPOffset<fb::ClaimedProfileEntry<'_>>> = model
        .entries
        .iter()
        .map(|(key, card)| {
            let key = fbb.create_string(key);
            let value = create_profile_card(&mut fbb, card);
            fb::ClaimedProfileEntry::create(
                &mut fbb,
                &fb::ClaimedProfileEntryArgs {
                    key: Some(key),
                    value: Some(value),
                },
            )
        })
        .collect();
    let entries = fbb.create_vector(&entry_offsets);
    let root = fb::ClaimedProfilesSnapshot::create(
        &mut fbb,
        &fb::ClaimedProfilesSnapshotArgs {
            entries: Some(entries),
        },
    );
    fb::finish_claimed_profiles_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode the SHARED generated `ProfileCard` table (`include`d from
/// `profile_card.fbs`) into a [`ProfileCardModel`] (mirrors
/// `profile_fb::profile_card_from_fb`; both read the same `pc::ProfileCard`).
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

/// Decode typed FlatBuffers bytes (as produced by [`encode_claimed_profiles`])
/// back into a [`ClaimedProfilesModel`]. Returns an error string on any
/// malformed input.
///
/// Promoted to unconditional `pub` (V-112 follow-up): `claim_profile` is the
/// component-owned profile-hydration verb, and the `claimed_profiles` typed
/// sidecar is its direct observable — out-of-tree Rust consumers (e.g. the
/// `nmp-defaults` `validate_claim_profile` example) read it through
/// `nmp_core::typed_projections`, mirroring the `decode_claimed_events` /
/// `decode_resolved_profiles` promotions from PR-B.
pub fn decode_claimed_profiles(bytes: &[u8]) -> Result<ClaimedProfilesModel, String> {
    if bytes.len() < 8 || !fb::claimed_profiles_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KCPR file identifier".to_string());
    }
    let root = fb::root_as_claimed_profiles_snapshot(bytes)
        .map_err(|e| format!("not a valid ClaimedProfilesSnapshot buffer: {e}"))?;

    let mut entries = Vec::new();
    if let Some(fb_entries) = root.entries() {
        entries.reserve(fb_entries.len());
        for entry in fb_entries.iter() {
            let key = entry.key().unwrap_or_default().to_string();
            let value = entry.value().map(profile_card_from_fb).unwrap_or_default();
            entries.push((key, value));
        }
    }
    Ok(ClaimedProfilesModel { entries })
}

#[cfg(test)]
#[path = "claimed_profiles_fb_tests.rs"]
mod tests;
