//! `nmp-wot` — client-side Web-of-Trust scoring for Nostr apps.
//!
//! The crate is intentionally local-only. It does not fetch relays, query
//! providers, or consume delegated assertion events. Callers feed it the graph
//! signals they already have: kind:3 follow lists and public kind:10000 mute
//! lists. The output is a deterministic personalized score that app
//! projections can use to rank nearby authors first and hide authors who are
//! heavily muted by the viewer's trusted graph.
//!
//! The algorithm is deliberately bounded for mobile/client use:
//!
//! - direct follows are always closest;
//! - trust propagates through follow edges up to `TrustConfig::max_depth`;
//! - high-degree follow lists are damped with `sqrt(out_degree)`;
//! - the viewer's own mute list is a hard hide;
//! - community mutes only auto-hide authors the viewer does not directly follow.

mod graph;
mod rank;
mod score;

pub use graph::{
    is_hex_pubkey, GraphStats, SignalGraph, SignalIngest, KIND_CONTACT_LIST, KIND_MUTE_LIST,
};
pub use rank::ScoredPubkey;
pub use score::{TrustConfig, TrustDecision, TrustIndex, TrustScore};
