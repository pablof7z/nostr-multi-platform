//! Typed FlatBuffers wire codec for the kernel-owned `"profile"` projection
//! (Tier-2 built-in), plus the shared [`ProfileCardModel`] row type the
//! `author_view` codec reuses.
//!
//! The authoritative FFI shape is the serde JSON the
//! `snapshot_projections_with_publish_cluster` helper inserts under
//! `"profile"`: the serialisation of `profile_card()` (a `ProfileCard` for the
//! active account). This module adds a **typed FlatBuffers** encoding of the
//! same shape, carried in the `typed_projections` sidecar (ADR-0037) ALONGSIDE
//! — never replacing — the generic `Value` projection.
//!
//! [`ProfileCardModel`] is built from the same `profile_card()` output the JSON
//! path serialises, in the same tick, so the two wire forms cannot diverge.
//! `ProfileCard` carries NO serde `skip_serializing_if`, so its three
//! `Option<String>` fields serialise as JSON `null`-when-`None` (key always
//! present); the typed buffer carries each presence as an explicit `has_*` flag.
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
#[path = "generated/profile_generated.rs"]
pub mod generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use generated::nmp::kernel as fb;
// Shared `ProfileCard` row type: `profile.fbs` `include`s `profile_card.fbs`, so
// the generated `ProfileCard` / `ProfileCardArgs` live in the crate-root
// `profile_card_generated` wrapper, NOT in this module's `generated::nmp::kernel`
// (that module only references them through its own private glob import).
use crate::profile_card_generated as pc;

/// Stable schema identifier carried in the typed-projection envelope. Equals the
/// snapshot key (ADR-0037 shared-keyspace contract).
pub const PROFILE_SCHEMA_ID: &str = "profile";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const PROFILE_FILE_IDENTIFIER: &[u8; 4] = b"KPRF";
/// Wire schema version. Bump on any breaking change to `profile.fbs`.
pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// A field-for-field mirror of one [`ProfileCard`](crate::kernel) — the shared
/// row type the `profile` and `author_view` codecs both encode (into their own
/// generated `ProfileCard` table). `Option<String>` fields are encoded as
/// `has_x` + value.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileCardModel {
    pub pubkey: String,
    pub npub: String,
    pub display_name: Option<String>,
    pub picture_url: Option<String>,
    pub nip05: String,
    pub about: String,
    pub lnurl: Option<String>,
}

// --- encode ---------------------------------------------------------------

/// Encode a [`ProfileCardModel`] into this module's generated `ProfileCard`
/// table, returning the nested offset for embedding in the root. Shared shape
/// with `author_view` (each codec calls its OWN generated `create`).
fn create_profile_card<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    card: &ProfileCardModel,
) -> WIPOffset<pc::ProfileCard<'a>> {
    let pubkey = fbb.create_string(&card.pubkey);
    // ADR-0032 / V-115: `npub` deprecated in schema; `ProfileCardArgs` no longer
    // has an `npub` field (flatc omits args for deprecated fields). The slot is
    // preserved in the vtable for wire compatibility with un-updated hosts.
    let display_name = card
        .display_name
        .as_ref()
        .map(|value| fbb.create_string(value));
    let picture_url = card
        .picture_url
        .as_ref()
        .map(|value| fbb.create_string(value));
    let nip05 = fbb.create_string(&card.nip05);
    let about = fbb.create_string(&card.about);
    let lnurl = card.lnurl.as_ref().map(|value| fbb.create_string(value));
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

/// Encode a [`ProfileCardModel`] to typed FlatBuffers bytes (with the `KPRF`
/// file identifier) under a `ProfileSnapshot` root.
#[must_use]
pub(crate) fn encode_profile(model: &ProfileCardModel) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();
    let card = create_profile_card(&mut fbb, model);
    let root = fb::ProfileSnapshot::create(&mut fbb, &fb::ProfileSnapshotArgs { card: Some(card) });
    fb::finish_profile_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode this module's generated `ProfileCard` table back into a
/// [`ProfileCardModel`]. Shared with the `author_view` test decoder shape, but
/// each codec reads its OWN generated `ProfileCard` type (hence not a free
/// function here — `author_view` mirrors this logic against its own bindings).
pub fn profile_card_from_fb(card: pc::ProfileCard<'_>) -> ProfileCardModel {
    ProfileCardModel {
        pubkey: card.pubkey().unwrap_or_default().to_string(),
        // ADR-0032 / V-115: `npub` deprecated in schema; no accessor generated.
        // Shells encode bech32 themselves. Field kept in model as empty string.
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

/// Decode typed FlatBuffers bytes (as produced by [`encode_profile`]) back into
/// a [`ProfileCardModel`]. Returns an error string on any malformed input.
pub fn decode_profile(bytes: &[u8]) -> Result<ProfileCardModel, String> {
    if bytes.len() < 8 || !fb::profile_snapshot_buffer_has_identifier(bytes) {
        return Err("missing KPRF file identifier".to_string());
    }
    let root = fb::root_as_profile_snapshot(bytes)
        .map_err(|e| format!("not a valid ProfileSnapshot buffer: {e}"))?;
    let card = root
        .card()
        .ok_or_else(|| "ProfileSnapshot missing card".to_string())?;
    Ok(profile_card_from_fb(card))
}

#[cfg(test)]
#[path = "profile_fb_tests.rs"]
mod tests;
