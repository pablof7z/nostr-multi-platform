//! Per-relay relay-pinned interest plan for a [`SearchRequest`].
//!
//! NIP-50 search is fanned out as one generic interest PER resolved search
//! relay, each carrying `InterestShape.relay_pin = Some(relay)` so the planner's
//! relay-pin lane (`case_e_relay_pinned`) routes it to exactly that relay
//! regardless of the author's NIP-65 mailboxes. The router still applies its
//! blocked-relay subtractive post-pass, so a pinned-but-blocked relay is
//! dropped by the same generic mechanism that guards every interest.
//!
//! `nmp-nip50` owns this fan-out policy; the substrate owns only the generic
//! relay-pin routing field.

use nmp_planner::InterestShape;

use crate::SearchRequest;

/// One relay-pinned generic interest for a search session: the request's base
/// [`InterestShape`] (kinds + bounded `search` + limit) with `relay_pin` set to
/// a single resolved search relay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayPinnedInterest {
    /// The relay this interest is pinned to.
    pub relay: String,
    /// The interest shape, with `relay_pin == Some(relay)`.
    pub shape: InterestShape,
}

/// Build the per-relay relay-pinned interest plan for `request` over the
/// already-resolved `relays` (see [`crate::resolve_search_relays`]).
///
/// One [`RelayPinnedInterest`] per relay; an empty `relays` slice yields an
/// empty plan (no network fan-out — the cache scan still runs upstream). The
/// base shape is [`SearchRequest::interest_shape`]; only `relay_pin` differs
/// across the returned interests, so each lands on a distinct registry slot
/// (the relay pin participates in the `InterestShape` hash) and closes
/// independently.
#[must_use]
pub fn search_relay_plan(request: &SearchRequest, relays: &[String]) -> Vec<RelayPinnedInterest> {
    let base = request.interest_shape();
    relays
        .iter()
        .map(|relay| {
            let mut shape = base.clone();
            shape.relay_pin = Some(relay.clone());
            RelayPinnedInterest {
                relay: relay.clone(),
                shape,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SearchScope, SearchTargets};

    fn request() -> SearchRequest {
        SearchRequest::new(
            "nostr",
            SearchScope::Users,
            SearchTargets::UserPreferred,
            Some(50),
        )
        .expect("query")
    }

    #[test]
    fn one_pinned_interest_per_relay() {
        let relays = vec!["wss://a/".to_string(), "wss://b/".to_string()];
        let plan = search_relay_plan(&request(), &relays);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].relay, "wss://a/");
        assert_eq!(plan[0].shape.relay_pin.as_deref(), Some("wss://a/"));
        assert_eq!(plan[1].shape.relay_pin.as_deref(), Some("wss://b/"));
        // Each carries the bounded search field + the scope kinds.
        assert_eq!(plan[0].shape.search.as_deref(), Some("nostr"));
        assert!(plan[0]
            .shape
            .kinds
            .contains(&nmp_kinds::KIND_PROFILE_METADATA));
    }

    #[test]
    fn empty_relays_yield_empty_plan() {
        assert!(search_relay_plan(&request(), &[]).is_empty());
    }

    #[test]
    fn distinct_relays_produce_distinct_shapes() {
        let relays = vec!["wss://a/".to_string(), "wss://b/".to_string()];
        let plan = search_relay_plan(&request(), &relays);
        assert_ne!(
            plan[0].shape, plan[1].shape,
            "relay_pin makes the shapes distinct"
        );
    }
}
