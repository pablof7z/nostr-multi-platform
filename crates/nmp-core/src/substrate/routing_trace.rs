//! Routing-trace observer substrate seam — V-51 phase 1 / V-75.
//!
//! See GitHub issue #968 for the V-51 rollout. This module ships
//! phase 1 (the observer trait + per-call summary structs) extended by V-75
//! (per-lane `RouteAttempt` records so the inspector can answer "why did
//! lanes 1–6 return empty and Lane 7 fire?"). Phase 2 adds the FFI/wasm
//! snapshot surface; phase 3 the Chirp inspector UI; phase 4 the
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
//! ## Per-lane attempt records (V-75)
//!
//! [`PublishTrace::attempts`] and [`SubscriptionTrace::attempts`] carry one
//! [`RouteAttempt`] per lane that ran during the generic routing algorithm, in
//! lane-order. Each record identifies the lane and its outcome (`Matched {
//! count }` or `Empty`). Together they let the V-51 inspector show the
//! empty-cause chain — e.g. "NIP-65 empty, Hint empty, UserConfigured empty →
//! AppRelayFallback resolved 1 relay". The `explicit_targets` short-circuit
//! path (Lane 5 / ClassRouted) skips all generic lanes; `attempts` is empty
//! in that case.
//!
//! ## Allocation contract (D8)
//!
//! Routers MUST gate the observer fan-out — including attempt accumulation —
//! on `Option<Arc<dyn ...>>::is_some()` so the no-observer path stays
//! zero-allocation per call. `Vec::new()` does not allocate until the first
//! push, so computing `let tracing_active = obs.is_some()` at the top of the
//! routing function and guarding each `attempts.push(...)` on that flag is
//! sufficient. A `Vec` with ≤ 6 entries (one per generic lane) fits without
//! heap allocation on most SSO-capable platforms.
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
//!
//! ## Derived computation note
//!
//! `attempts` is *derived* during routing (not lifted verbatim from the call
//! inputs). This is an intentional V-75 extension: without the per-lane
//! empty-set signal the inspector cannot distinguish "lane not applicable"
//! from "lane tried but empty".

use super::routing::{Pubkey, RoutedRelaySet};

// ─── RouteAttempt — per-lane observability record (V-75) ─────────────────────

/// Identifies which routing lane ran during a `route_publish` or
/// `route_subscription` call. Corresponds to the generic lanes in
/// `docs/architecture/crate-boundaries.md` §3.1.
///
/// `AppRelayFallback` is a dedicated variant to signal explicitly "Lane 7
/// fired because all prior lanes produced empty sets" — the core V-75
/// diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutingLane {
    /// Lane 1 — per-author NIP-65 mailbox (write set for publish, read set
    /// for subscribe).
    Nip65,
    /// Lane 2 — relay-hint URLs from event tags (publish) or
    /// `interest.hints` (subscribe).
    Hint,
    /// Lane 3 — provenance (subscribe-only: re-fetch from the relay a prior
    /// event was observed on).
    Provenance,
    /// Lane 4 — user-configured relays (active-account read / write).
    UserConfigured,
    /// Lane 6 — operator indexer relays (always-on for discovery kinds).
    Indexer,
    /// Lane 7 — app-relay fallback. Fires **only** when all prior generic
    /// lanes (1–6) produced an empty set.
    AppRelayFallback,
}

/// Outcome of a single lane's attempt during a routing call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneOutcome {
    /// The lane contributed at least one relay URL that passed the
    /// blocked-relay and admission-policy filters. `count` is the number of
    /// admissible URLs the lane processed — it may include URLs that were
    /// already in the set from an earlier lane (stacking semantics), so
    /// this is an "attempted-and-admitted" count, not a net-new count.
    Matched {
        /// Number of admissible relay URLs contributed by this lane.
        count: usize,
    },
    /// The lane ran but all candidate URLs were either absent (empty NIP-65
    /// cache entry, no hint tags on the event, no active-account relays
    /// configured, etc.) or filtered out by blocked-relay / admission policy.
    Empty,
}

/// One lane's attempt + outcome, collected into [`PublishTrace::attempts`]
/// and [`SubscriptionTrace::attempts`] in lane-order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteAttempt {
    /// Which lane ran.
    pub lane: RoutingLane,
    /// What it produced.
    pub outcome: LaneOutcome,
}

// ─── PublishTrace / SubscriptionTrace ────────────────────────────────────────

/// Log-safe summary of a `route_publish` call. Constructed by the router
/// from data it had on the stack.
///
/// `attempts` is the V-75 extension: one [`RouteAttempt`] per lane that ran
/// during the generic algorithm, in lane-order. Empty when
/// `explicit_targets_set` is `true` (the generic algorithm was skipped).
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
    /// Per-lane attempt records for the generic algorithm (V-75). Empty when
    /// `explicit_targets_set` is `true`. Ordered lane 1 → lane 7; the last
    /// entry is `AppRelayFallback` when Lane 7 fired.
    pub attempts: Vec<RouteAttempt>,
}

/// Log-safe summary of a `route_subscription` call. Constructed by the router
/// from data it had on the stack.
///
/// `attempts` is the V-75 extension: one [`RouteAttempt`] per lane that ran
/// during the generic algorithm, in lane-order. Empty when
/// `explicit_targets_set` is `true`.
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
    /// Per-lane attempt records for the generic algorithm (V-75). Empty when
    /// `explicit_targets_set` is `true`. Ordered lane 1 → lane 7; the last
    /// entry is `AppRelayFallback` when Lane 7 fired.
    pub attempts: Vec<RouteAttempt>,
}

// ─── RoutingTraceObserver ─────────────────────────────────────────────────────

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

// ─── truncate_event_id ───────────────────────────────────────────────────────

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
            attempts: vec![],
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
            attempts: vec![],
        };
        let _ = t.clone();
        let _ = format!("{t:?}");
    }

    #[test]
    fn route_attempt_is_clone_copy_and_debug() {
        let a = RouteAttempt {
            lane: RoutingLane::AppRelayFallback,
            outcome: LaneOutcome::Matched { count: 2 },
        };
        let b = a;
        let _ = format!("{b:?}");
        assert_eq!(a, b);
    }
}
