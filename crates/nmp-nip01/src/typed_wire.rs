//! Typed FlatBuffers wire encoding for `nmp_nip01::ModularTimelineSnapshot`.
//!
//! This is the nmp-nip01-owned typed projection of the assembled home-feed
//! snapshot: one `nmp.nip01.timeline.ModularTimelineSnapshot` buffer (file
//! identifier `NFTS`) carrying render-ready cards, typed content-render data,
//! typed nmp-content trees, and an embedded typed nmp-feed window.
//!
//! Relates to ADR-0037 (typed FlatBuffers runtime projections) and ADR-0032
//! (raw-data projection doctrine). ADR-0037's authorized pilot is the
//! `nmp.feed.home` projection. This schema owns the nmp-nip01 timeline/card
//! payload; nmp-feed owns the cursor/page/window payload embedded in it.
//!
//! ## Parity, not simplification
//!
//! Every field of the serde [`ModularTimelineSnapshot`] survives the round
//! trip: blocks, cards, author display, relation counts, content render facts,
//! repost attribution, and the optional feed window.
//!
//! ## Typed sub-payloads
//!
//! `content_tree` is embedded as the typed `nmp-content` FlatBuffers buffer
//! (`schema_id "nmp.content.tree"`, file identifier `NFCT`). Feed
//! page/cursor/metrics travel as the typed `nmp-feed` `FeedWindow` buffer
//! (`schema_id "nmp.feed.window"`, file identifier `NFWM`). `content_render`
//! is encoded as native nmp-nip01 tables in this schema.
//!
//! ## Regenerating the bindings
//!
//! The checked-in bindings in `wire/generated/timeline_snapshot_generated.rs`
//! are produced by `flatc` from `schema/timeline_snapshot.fbs`. Regenerate
//! only with the workspace FlatBuffers pin (`25.12.19`), enforced by
//! `ci/check-flatbuffers-version-pins.sh`:
//!
//! ```sh
//! flatc --rust -o crates/nmp-nip01/src/wire/generated \
//!       crates/nmp-nip01/schema/timeline_snapshot.fbs
//! ```

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "wire/generated/timeline_snapshot_generated.rs"]
mod timeline_snapshot_generated;

mod decode;
mod encode;

use crate::timeline_projection::ModularTimelineSnapshot;
pub(super) use timeline_snapshot_generated::nmp::nip_01 as fb;

/// Stable projection identifier this wire shape projects into.
pub const SCHEMA_ID: &str = "nmp.nip01.timeline";

/// FlatBuffers file identifier for a `ModularTimelineSnapshot` root buffer.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NFTS";

/// Schema version of the typed timeline-snapshot payload. Bump on any breaking
/// field change. Mirrors `ModularTimelineSnapshot.schema_version` in the `.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

/// Encode a [`ModularTimelineSnapshot`] as one typed FlatBuffers
/// `ModularTimelineSnapshot` buffer with the `NFTS` file identifier.
#[must_use]
pub fn encode_modular_timeline_snapshot(snapshot: &ModularTimelineSnapshot) -> Vec<u8> {
    encode::encode_modular_timeline_snapshot(snapshot)
}

/// Decode a typed FlatBuffers `ModularTimelineSnapshot` buffer back into the
/// owned [`ModularTimelineSnapshot`]. Returns a human-readable error string on
/// any malformed-buffer or missing-required-field condition.
pub fn decode_modular_timeline_snapshot(bytes: &[u8]) -> Result<ModularTimelineSnapshot, String> {
    decode::decode_modular_timeline_snapshot(bytes)
}

#[cfg(test)]
#[path = "typed_wire/tests.rs"]
mod tests;
