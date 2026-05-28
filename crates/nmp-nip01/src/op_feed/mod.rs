//! `op_feed` — the NIP-10 instance of the generic OP-centric home-feed engine
//! (`nmp_feed::RootIndexedFeed`), V-80 rung 5 (Stage 3b).
//!
//! The generic engine in `nmp-feed` (rung 3) knows no protocol convention. This
//! module binds it to NIP-10:
//!
//! * [`Nip10ReplyAttribution`] — the `AttributionPayload` instance
//!   (`Profile = ProfileDisplay`), built from a followed author's NIP-10 reply.
//! * [`register_op_feed`] — constructs
//!   `RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution, TimelineEventCard>`
//!   and returns it for the composition root (rung 6) to register against an
//!   `NmpApp`. **This rung ships the instance unwired in production** — only
//!   tests register it, so Chirp's existing `ModularTimelineProjection` keeps
//!   ownership of `"nmp.feed.home"` until rung 7.
//! * [`build_actor_claim_sink`] — turns the engine's `ClaimRequest` stream into
//!   `ActorCommand::ClaimEvent` / `ReleaseEvent` dispatches, encoding the
//!   `ThreadPointer` as a `nostr:` URI (`nevent` / `naddr`).
//!
//! See `docs/perf/op-centric-feed-architecture.md` §3-A/B/C/L.

mod attribution;
pub mod typed_wire;
mod wiring;

pub use attribution::Nip10ReplyAttribution;
pub use typed_wire::{
    decode_op_feed_snapshot, encode_op_feed_snapshot, OpFeedSnapshot, OP_FEED_FILE_IDENTIFIER,
    OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION,
};
pub use wiring::{
    build_actor_claim_sink, register_op_feed, ActorCommandDispatch, OpFeedEngine,
    OP_FEED_SNAPSHOT_KEY,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
