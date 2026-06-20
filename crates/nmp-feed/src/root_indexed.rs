//! Generic OP-centric home-feed engine.
//!
//! The home feed is a stream of **thread roots only**; a followed author's
//! reply to a non-followed root surfaces that root with the reply as
//! *attribution* metadata. This module owns the substrate-generic state
//! machine ([`RootIndexedFeed`]) and its value types ([`AttributionPayload`],
//! [`RootCard`], [`RootFeedSnapshot`]). Protocol knowledge (what a reply is,
//! what a follow is, or which secondary data a renderer wants) lives entirely
//! in the instance crate or mounted component that supplies `R`, `A`, and the
//! closures.
//!
//! See `docs/decisions/0035-generic-root-indexed-feed-engine.md` and
//! `docs/perf/op-centric-feed-architecture.md` §3 for the full design, and

mod attribution;
mod card;
mod engine;

#[cfg(test)]
mod tests;

pub use attribution::AttributionPayload;
pub use card::{RootCard, RootFeedSnapshot};
pub use engine::{
    CardBuilder, EventGate, EventLookup, FollowPredicate, RootIndexedFeed, MAX_ATTRIBUTION_PER_ROOT,
};
