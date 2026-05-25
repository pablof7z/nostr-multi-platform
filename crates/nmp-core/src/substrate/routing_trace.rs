//! Routing-trace observer substrate seam — V-51 phase 1.
//!
//! See `docs/BACKLOG.md` §V-51 for the four-phase rollout. This module ships
//! phase 1 only: the observer trait + two log-safe payload structs the router
//! fires on every `route_publish` / `route_subscription` call. Phase 2 adds
//! the FFI/wasm snapshot surface; phase 3 the Chirp inspector UI; phase 4 the
//! validation-CLI subcommand. The bounded ring buffer projection that
//! consumes these callbacks lives in `crate::kernel::routing_trace`.
//!
//! ## Why this trait
//!
//! [`super::RoutedRelaySet`] already attributes every resolved URL to one or
//! more [`super::RoutingSource`] lanes — that data exists at the router call
//! site but never leaves it. Without an observation seam there is no way for
//! an app (Chirp, the validation harness, a debug tool) to answer "why did
//! event Y go to relay B?". This trait *is* the seam.
//!
//! ## Allocation contract (D8)
//!
//! Routers MUST gate the observer fan-out on `Option<Arc<dyn ...>>::is_some()`
//! so the no-observer path stays zero-allocation per call. When an observer
//! IS installed, the per-call summary structs ([`PublishTrace`],
//! [`SubscriptionTrace`]) carry only fields the router already had on the
//! stack — no derived computation, no event-content copy.
//!
//! ## Log-safety
//!
//! Neither [`PublishTrace`] nor [`SubscriptionTrace`] carries:
//! - event content (`content` field)
//! - decrypted DM plaintext
//! - private keys or any secret material
//! - tags beyond the lane attribution already in `RoutedRelaySet`
//!
//! `event_id_short` is truncated to the first 12 hex chars so a routing trace
//! is safe to write to a debug log or send over the wire to a remote inspector
//! without leaking the full event identity. `author` is the bare public key
//! (already on the wire as the event's `pubkey` field).

use super::routing::{Pubkey, RoutedRelaySet};

/// Log-safe summary of a `route_publish` call. Constructed by the router
/// from data it already had on the stack; never derived.
#[derive(Clone, Debug)]
pub struct PublishTrace {
    /// Event kind (`UnsignedEvent::kind`). Always present.
    pub kind: u32,
    /// Author pubkey (`UnsignedEvent::pubkey`). Always present.
    pub author: Pubkey,
    /// Truncated event id (first 12 hex chars), or `None` for unsigned events
    /// where the id has not yet been computed (publish-side: the router runs
    /// BEFORE signing per `OutboxRouter` doc-comment).
    pub event_id_short: Option<String>,
    /// Whether `RoutingContext::explicit_targets` was populated, i.e. whether
    /// the §3.4 override seam fired. When `true` the resolved relay set is
    /// the explicit-targets list (minus blocked-relay hits); when `false` the
    /// resolved set came from the generic algorithm.
    pub explicit_targets_set: bool,
}

/// Log-safe summary of a `route_subscription` call. Constructed by the router
/// from data it already had on the stack; never derived.
#[derive(Clone, Debug)]
pub struct SubscriptionTrace {
    /// The opaque interest id (`LogicalInterest::id.0`).
    pub interest_id: u64,
    /// Kinds the interest filters on (`InterestShape::kinds`). Bounded by the
    /// interest shape; in practice ≤ a handful per interest.
    pub kinds: Vec<u32>,
    /// Number of authors in the interest shape. Bare count (not the list)
    /// because the list can be large for follow-feed interests; the per-URL
    /// `RoutingSource::Nip65 { direction: Read }` attribution already tells
    /// the consumer which author drove which URL.
    pub authors_count: usize,
    /// Whether `RoutingContext::explicit_targets` was populated (see
    /// [`PublishTrace::explicit_targets_set`]).
    pub explicit_targets_set: bool,
}

/// Substrate trait — fired by `OutboxRouter` impls after every successful
/// route resolution so a downstream projection / inspector can answer
/// "why did event Y go to relay B?".
///
/// `Send + Sync` so the kernel can hold the observer as `Arc<dyn ...>` and
/// hand router impls a clone for fan-out at the router call site.
///
/// Routers MUST NOT fire the observer on `Err(RoutingError::*)` returns —
/// the no-relays-resolved case is already surfaced via `CompiledPlan::
/// unroutable_authors` and re-firing the observer there would just duplicate
/// that signal in two projections.
pub trait RoutingTraceObserver: Send + Sync {
    /// Fired after a successful `route_publish` resolution. `routed` is the
    /// `&RoutedRelaySet` the router is about to return — observers MUST NOT
    /// mutate it (the borrow checker enforces the immutable share).
    fn on_publish(&self, summary: PublishTrace, routed: &RoutedRelaySet);

    /// Fired after a successful `route_subscription` resolution. Same
    /// no-mutation contract as `on_publish`.
    fn on_subscription(&self, summary: SubscriptionTrace, routed: &RoutedRelaySet);
}

/// Truncate a 64-char lowercase hex event id to its first 12 chars for
/// log-safe inclusion in a [`PublishTrace`]. Callers that already have
/// the id as bytes are expected to hex-encode + slice; this helper covers
/// the str-keyed case the router has when it reads from `UnsignedEvent`.
///
/// `None` in, `None` out — the publish-side router runs BEFORE signing, so
/// the unsigned event has no id yet; the caller passes `None` and the
/// projection records "id was not yet computed". The subscription-side has
/// no event id at all and ignores this helper.
#[must_use]
pub fn truncate_event_id(id: Option<&str>) -> Option<String> {
    id.map(|s| s.chars().take(12).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_event_id_takes_first_twelve_chars() {
        let id = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        assert_eq!(truncate_event_id(Some(id)), Some("abcdef012345".into()));
    }

    #[test]
    fn truncate_event_id_passes_through_short_input() {
        let id = "abcd";
        assert_eq!(truncate_event_id(Some(id)), Some("abcd".into()));
    }

    #[test]
    fn truncate_event_id_none_passes_through() {
        assert_eq!(truncate_event_id(None), None);
    }

    #[test]
    fn publish_trace_is_clone_and_debug() {
        let t = PublishTrace {
            kind: 1,
            author: "alice".into(),
            event_id_short: Some("abcdef012345".into()),
            explicit_targets_set: false,
        };
        let _ = t.clone();
        let _ = format!("{t:?}");
    }

    #[test]
    fn subscription_trace_is_clone_and_debug() {
        let t = SubscriptionTrace {
            interest_id: 42,
            kinds: vec![1, 6, 7],
            authors_count: 5,
            explicit_targets_set: true,
        };
        let _ = t.clone();
        let _ = format!("{t:?}");
    }
}
