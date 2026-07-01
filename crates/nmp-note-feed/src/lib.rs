//! `nmp-note-feed` — reusable note-feed composition.
//!
//! This crate owns concrete feed output for note timelines: active-follow
//! feeds, app-owned timeline feeds, author feeds, and thread feeds. It composes
//! lower-level NIP facts (`nmp-nip01` kind:1 / NIP-10 and `nmp-nip18` reposts)
//! with generic `nmp-feed` mechanics, then emits feed-owned typed wire. Lower
//! protocol crates do not own row/card render contracts.

mod card_payload;
pub mod flat_feed;
mod note_item;
pub mod op_feed;

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "wire/generated/op_feed_generated.rs"]
mod op_feed_generated;

pub use flat_feed::{
    author_feed_predicate, author_feed_shape, thread_feed_predicate, thread_feed_shape, FlatFeed,
    FlatFeedPredicate,
};
pub use note_item::{NoteFeedItem, RepostAttribution};
pub use op_feed::{
    decode_op_feed_snapshot, encode_op_feed_snapshot, op_feed_observer, register_op_feed,
    register_op_feed_with_admission, Nip10ReplyAttribution, OpFeedEngine, OpFeedObserver,
    OpFeedSnapshot, OP_FEED_FILE_IDENTIFIER, OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION,
};

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
