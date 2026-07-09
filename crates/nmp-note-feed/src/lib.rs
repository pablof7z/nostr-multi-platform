//! `nmp-note-feed` — thin protocol-composition adapter (post-demolition).
//!
//! # This crate was demolished — TODO(#3082)
//!
//! It USED to own a baked, note-and-reply-specific feed ENGINE (the
//! `RootIndexed` OP-feed instance, `NoteFeedResolver`, `Nip10ReplyAttribution`),
//! an authoritative note-only row (`NoteFeedItem`), and a note-only FlatBuffers
//! wire (`OpFeedSnapshot` / NNFS). That baked one app's timeline surface as the
//! framework default and read the event cache synchronously (the order-dependent
//! "cache-luck" bug, #3083). All of that is DELETED.
//!
//! What remains is a small set of app/protocol KNOBS for the generic
//! `nmp_feed::FlatFeed<nmp_feed::FeedRow>` engine:
//!
//!   * [`feed_row_builder`] — identity (NIP-18 repost → target id) + row build;
//!   * [`timeline_merge`] — repost/target merge;
//!   * [`author_feed_predicate`] / [`thread_feed_predicate`] and their shapes.
//!
//! The engine, the row (`FeedRow`), the frozen FlatBuffers wire, and the
//! snapshot type all live in `nmp-feed` (`nmp_feed::typed_wire`) — this crate
//! no longer re-exports a wire (#3082 shape freeze; the provisional serde-JSON
//! `NFRW` wire is deleted, not moved). Reposts carry a `TypedRef` render-only
//! pointer resolved lazily via `resolve_ref` — reply-rollup is no longer a
//! framework behavior.
//!
//! TODO(#3082): this crate may be deleted entirely once its knobs are relocated
//! into the composition root (`nmp-feed-session`).

pub mod flat_feed;

pub use flat_feed::{
    author_feed_predicate, author_feed_shape, feed_row_builder, no_group_context,
    thread_feed_predicate, thread_feed_shape, timeline_merge, FeedRowGroupContext,
};

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
