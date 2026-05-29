//! Generic OP-centric home-feed engine.
//!
//! The home feed is a stream of **thread roots only**; a followed author's
//! reply to a non-followed root surfaces that root with the reply as
//! *attribution* metadata. This module owns the substrate-generic state
//! machine ([`RootIndexedFeed`]) and its value types ([`AttributionPayload`],
//! [`ClaimRequest`], [`RootCard`], [`RootFeedSnapshot`]). Protocol knowledge
//! (what a reply is, what a follow is, how a profile decodes) lives entirely
//! in the instance crate that supplies `R`, `A`, and the closures.
//!
//! See `docs/decisions/0035-generic-root-indexed-feed-engine.md` and
//! `docs/perf/op-centric-feed-architecture.md` §3 for the full design, and
//! `docs/BACKLOG.md` V-81 for the release-signal timing decision implemented
//! in [`RootIndexedFeed::on_event_claim_released`].

mod attribution;
mod card;
mod claim;
mod engine;

#[cfg(test)]
mod tests;

pub use attribution::AttributionPayload;
pub use card::{RootCard, RootFeedSnapshot};
pub use claim::ClaimRequest;
pub use engine::{
    CardBuilder, ClaimSink, EventGate, EventLookup, FollowPredicate, ProfileDetector,
    RootIndexedFeed, MAX_ATTRIBUTION_PER_ROOT,
};
