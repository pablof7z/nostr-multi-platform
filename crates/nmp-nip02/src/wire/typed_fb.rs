//! Typed FlatBuffers wire codec for [`crate::projection::FollowListSnapshot`].
//!
//! The authoritative wire shape of the `"nmp.follow_list"` projection is the
//! serde JSON of the follow-list snapshot (registered via
//! `register_snapshot_projection` in [`crate::register_follow_state_runtime`],
//! called by the composition root against the `nmp-uniffi` native binding
//! surface). This module adds a **typed FlatBuffers** encoding of the same snapshot — a
//! self-describing, schema-versioned, language-neutral binary the host
//! platforms (Swift / Kotlin / TypeScript) can decode with generated accessors
//! instead of JSON reflection. It is a sidecar codec: the serde shape stays
//! authoritative; this is the typed payload carried in the `typed_projections`
//! sidecar (ADR-0072, `crates/nmp-core/schema/nmp_update.fbs`).
//!
//! Note the deliberate key/schema_id split (same convention wallet used): the
//! snapshot-tree registration key is `"nmp.follow_list"`, while this typed
//! payload's stable identity ([`SCHEMA_ID`]) is `"nmp.nip02.follow_list"`.
//!
//! The schema (`crates/nmp-nip02/schema/follow_list.fbs`) mirrors the Rust
//! snapshot field-for-field: `{ follows: [FollowEntry { pubkey }] }` carrying
//! only the raw hex pubkey (presentation formatting is a host concern).
//!
//! Honours D6 (no panics): decode returns `Err(String)` on any malformed input;
//! there are no `unwrap`/`expect`/panicking-index operations on the decode path.

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This `allow` block scopes the relaxation to the
// single generated module — no hand-written code in this file uses `unsafe`.
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
#[path = "generated/follow_list_generated.rs"]
pub mod generated;

use flatbuffers::WIPOffset;

use generated::nmp::nip_02 as fb;

use crate::projection::{FollowEntry, FollowListSnapshot};

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.nip02.follow_list";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NF02";
/// Wire schema version. Bump on any breaking change to `follow_list.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- encode ---------------------------------------------------------------

/// Encode a [`FollowListSnapshot`] to typed FlatBuffers bytes (with the `NF02`
/// file identifier).
#[must_use]
pub fn encode_follow_list(snapshot: &FollowListSnapshot) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();

    let follows: Vec<WIPOffset<fb::FollowEntry<'_>>> = snapshot
        .follows
        .iter()
        .map(|entry| {
            let pubkey = fbb.create_string(&entry.pubkey);
            fb::FollowEntry::create(
                &mut fbb,
                &fb::FollowEntryArgs {
                    pubkey: Some(pubkey),
                },
            )
        })
        .collect();
    let follows = fbb.create_vector(&follows);

    let root = fb::FollowListSnapshot::create(
        &mut fbb,
        &fb::FollowListSnapshotArgs {
            follows: Some(follows),
        },
    );
    fb::finish_follow_list_snapshot_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_follow_list`]) back
/// into a [`FollowListSnapshot`]. Returns an error string on any malformed
/// input.
pub fn decode_follow_list(bytes: &[u8]) -> Result<FollowListSnapshot, String> {
    if bytes.len() < 8 || !fb::follow_list_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NF02 file identifier".to_string());
    }
    let root = fb::root_as_follow_list_snapshot(bytes)
        .map_err(|e| format!("not a valid FollowListSnapshot buffer: {e}"))?;

    let mut follows = Vec::new();
    if let Some(entries) = root.follows() {
        for entry in entries.iter() {
            let pubkey = entry
                .pubkey()
                .ok_or_else(|| "FollowEntry.pubkey: missing required string".to_string())?
                .to_string();
            follows.push(FollowEntry { pubkey });
        }
    }
    Ok(FollowListSnapshot { follows })
}

#[cfg(test)]
#[path = "typed_fb_tests.rs"]
mod tests;
