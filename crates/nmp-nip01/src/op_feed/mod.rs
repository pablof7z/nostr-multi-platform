//! `op_feed` — the NIP-10 instance of the generic OP-centric home-feed engine
//! (`nmp_feed::RootIndexedFeed`), V-80 rung 5 (Stage 3b).
//!
//! The generic engine in `nmp-feed` (rung 3) knows no protocol convention. This
//! module binds it to NIP-10:
//!
//! * [`Nip10ReplyAttribution`] — the `AttributionPayload` instance
//!   built from a followed author's NIP-10 reply.
//! * [`register_op_feed`] — constructs
//!   `RootIndexedFeed<Nip10Resolver, Nip10ReplyAttribution, TimelineEventCard>`
//!   and returns it for the composition root (rung 6) to register against an
//!   `NmpApp`.
//! * [`FeedEmissionState`] — ADR-0055 Rung 6 S1: trap-proof per-tick emission
//!   state that omits an unchanged feed frame when the host has declared
//!   incremental-apply capability (exact byte equality, monotonic rev).
//!
//! See `docs/perf/op-centric-feed-architecture.md` §3-A/B/C/L.

mod attribution;
pub mod emission_state;
pub mod typed_wire;
mod wiring;

pub use attribution::Nip10ReplyAttribution;
pub use emission_state::{FeedEmissionState, FrameIdentity};
pub use typed_wire::{
    decode_op_feed_snapshot, encode_op_feed_snapshot, OpFeedSnapshot, OP_FEED_FILE_IDENTIFIER,
    OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION,
};
pub use wiring::{register_op_feed, OpFeedEngine, OP_FEED_SNAPSHOT_KEY};

#[cfg(test)]
mod test_support;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
