//! PROVISIONAL typed wire for the generic feed-row snapshot — TODO(#3082).
//!
//! The note-only NNFS FlatBuffers wire (`op_feed.fbs` / `NoteFeedItem` /
//! `OpFeedSnapshot`) was demolished. Its replacement is a kind-agnostic
//! `RootFeedSnapshot<FeedRow, ()>`. The FINAL wire is an OPEN decision (#3082):
//! the exact `FeedRow` field set and typed-context union are not yet frozen, so
//! this crate does NOT yet ship a regenerated FlatBuffers schema.
//!
//! Until #3082 freezes the shape, the typed sidecar encodes via `serde_json`
//! under a new file identifier (`NFRW`). This is deliberately provisional:
//!
//!   * the iOS/Android/web shells CANNOT decode this until the FlatBuffers wire
//!     is regenerated (they must be updated in the same follow-up);
//!   * do NOT treat `NFRW` as a stable wire — it exists only to keep the native
//!     runtime's typed-projection lane structurally wired during the demolition.
//!
//! When #3082 settles: add `crates/nmp-feed/schema/feed_row.fbs`, regenerate via
//! `ci/regenerate-flatbuffers.sh`, and replace the serde encode/decode below.

use nmp_feed::{FeedRow, RootFeedSnapshot};

/// The kind-agnostic snapshot this crate encodes. `A = ()` — flat feeds carry
/// no attribution rollup.
pub type FeedRowSnapshot = RootFeedSnapshot<FeedRow>;

pub const FEED_ROW_SCHEMA_ID: &str = "nmp.feed.feed_row";
/// Provisional file identifier — NOT a frozen wire (see module docs, #3082).
pub const FEED_ROW_FILE_IDENTIFIER: &[u8; 4] = b"NFRW";
pub const FEED_ROW_SCHEMA_VERSION: u32 = 1;

/// PROVISIONAL serde-JSON encode (TODO(#3082): replace with typed FlatBuffers).
#[must_use]
pub fn encode_feed_row_snapshot(snapshot: &FeedRowSnapshot) -> Vec<u8> {
    serde_json::to_vec(snapshot).unwrap_or_default()
}

/// PROVISIONAL serde-JSON decode (TODO(#3082): replace with typed FlatBuffers).
pub fn decode_feed_row_snapshot(bytes: &[u8]) -> Result<FeedRowSnapshot, String> {
    serde_json::from_slice(bytes).map_err(|err| err.to_string())
}
