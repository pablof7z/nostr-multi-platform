//! NIP-51 active-account interest builders for bookmark and mute lists.
//!
//! The host-driven counterpart to `nmp_nip57::self_zap_receipts_interest`
//! for NIP-51 replaceable lists authored by the active account. A host shell
//! wires each through a runtime controller so the kernel learns nothing about
//! NIP-51 nouns — it just routes generic [`LogicalInterest`] values exactly
//! the way it routes any other interest.
//!
//! # Why `Global + Nip65ReadRelays`
//!
//! Both interests carry `authors=[pubkey]` and route through the planner's
//! **Case A** (explicit authors → outbox / write relays) at
//! `crates/nmp-planner/src/compiler/partition/case_a_authors.rs`.
//! `InterestScope::Global` ensures the full relay-lane set is evaluated;
//! `PTagRouting::Nip65ReadRelays` prevents fail-closed DM-relay routing.
//!
//! # Single-slot semantics
//!
//! Each interest id is pubkey-invariant on purpose: the controller withdraws
//! the prior interest by id and pushes a fresh one on account switch, so the
//! kernel never accumulates one standing subscription per ever-active pubkey.
//! Mirrors the NIP-57 zap-receipts slot pattern.

use nmp_core::substrate::ViewDependencies;
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest, PTagRouting};

use nmp_kinds::{KIND_BOOKMARK_LIST, KIND_MUTE_LIST, KIND_SEARCH_RELAYS};

// ── Bookmark list (kind:10003) ────────────────────────────────────────────────

/// Stable id for the active-account-owned bookmark-list interest.
///
/// The id is intentionally independent of the pubkey so an account switch
/// replaces the prior `authors` filter instead of accumulating one long-lived
/// subscription per account. Mirrors
/// [`nmp_nip57::self_zap_receipts_interest_id`] line for line.
#[must_use]
pub fn active_bookmark_list_interest_id() -> InterestId {
    InterestId(nmp_planner::stable_hash::stable_hash64(
        "nmp.nip51.active_bookmark_list",
    ))
}

/// Tailing [`LogicalInterest`] for kind:10003 `authors=[pubkey]` bookmark lists —
/// the subscription a host pushes (via a runtime controller) so a
/// [`BookmarkListProjection`](crate::BookmarkListProjection) actually receives
/// the active account's bookmark events.
///
/// Shape:
/// - `lifecycle = Tailing`
/// - `scope = Global`
/// - `kinds = [10003]`
/// - `authors = [pubkey]`
/// - `p_tag_routing = Nip65ReadRelays`
#[must_use]
pub fn active_bookmark_list_interest(pubkey: &str) -> LogicalInterest {
    let deps = ViewDependencies {
        kinds: vec![KIND_BOOKMARK_LIST],
        authors: vec![pubkey.to_string()],
        ..Default::default()
    };
    let mut interest = deps.into_logical_interest(
        active_bookmark_list_interest_id(),
        InterestScope::Global,
        InterestLifecycle::Tailing,
    );
    interest.shape.p_tag_routing = PTagRouting::Nip65ReadRelays;
    interest
}

// ── Mute list (kind:10000) ────────────────────────────────────────────────────

/// Stable id for the active-account-owned mute-list interest.
///
/// Pubkey-invariant so an account switch replaces the prior subscription
/// rather than accumulating one per ever-active pubkey.
#[must_use]
pub fn active_mute_list_interest_id() -> InterestId {
    InterestId(nmp_planner::stable_hash::stable_hash64(
        "nmp.nip51.active_mute_list",
    ))
}

/// Tailing [`LogicalInterest`] for kind:10000 `authors=[pubkey]` mute lists.
///
/// Shape:
/// - `lifecycle = Tailing`
/// - `scope = Global`
/// - `authors = [pubkey]`
/// - `kinds = [10000]`
/// - `p_tag_routing = Nip65ReadRelays`
/// - `is_indexer_discovery = true` — enables the cold-start bootstrap
///   fallback to `bootstrap_indexer_relays` when the active account's NIP-65
///   mailbox is not yet cached (mirrors the `SELF_KINDS_TAILING` flag in
///   `startup.rs` that this interest replaces).
#[must_use]
pub fn active_mute_list_interest(pubkey: &str) -> LogicalInterest {
    let deps = ViewDependencies {
        kinds: vec![KIND_MUTE_LIST],
        authors: vec![pubkey.to_string()],
        ..Default::default()
    };
    let mut interest = deps.into_logical_interest(
        active_mute_list_interest_id(),
        InterestScope::Global,
        InterestLifecycle::Tailing,
    );
    interest.shape.p_tag_routing = PTagRouting::Nip65ReadRelays;
    // Bootstrap fallback: when the active account has no NIP-65 mailbox cached
    // yet AND no app_relays are configured, the planner's Case A routes the
    // interest to `bootstrap_indexer_relays` instead of marking the author
    // `unroutable`. Same flag that `SELF_KINDS_TAILING` set in startup.rs.
    interest.is_indexer_discovery = true;
    interest
}

// ── Search-relay list (kind:10007) ───────────────────────────────────────────

/// Stable id for the active-account-owned search-relay-list interest.
///
/// The id is intentionally independent of the pubkey so an account switch
/// replaces the prior `authors` filter instead of accumulating one long-lived
/// subscription per account. Mirrors [`active_bookmark_list_interest_id`].
#[must_use]
pub fn active_search_relay_list_interest_id() -> InterestId {
    InterestId(nmp_planner::stable_hash::stable_hash64(
        "nmp.nip51.active_search_relay_list",
    ))
}

/// Tailing [`LogicalInterest`] for kind:10007 `authors=[pubkey]` search-relay
/// lists — the subscription a host pushes (via a runtime controller) so a
/// [`SearchRelayListProjection`](crate::SearchRelayListProjection) receives
/// the active account's search-relay events.
///
/// Shape:
/// - `lifecycle = Tailing`
/// - `scope = Global`
/// - `kinds = [10007]`
/// - `authors = [pubkey]`
/// - `p_tag_routing = Nip65ReadRelays`
/// - `is_indexer_discovery = true` — cold-start bootstrap fallback
#[must_use]
pub fn active_search_relay_list_interest(pubkey: &str) -> LogicalInterest {
    let deps = ViewDependencies {
        kinds: vec![KIND_SEARCH_RELAYS],
        authors: vec![pubkey.to_string()],
        ..Default::default()
    };
    let mut interest = deps.into_logical_interest(
        active_search_relay_list_interest_id(),
        InterestScope::Global,
        InterestLifecycle::Tailing,
    );
    interest.shape.p_tag_routing = PTagRouting::Nip65ReadRelays;
    // Bootstrap fallback: when the active account has no NIP-65 mailbox cached
    // yet AND no app_relays are configured, the planner's Case A routes the
    // interest to `bootstrap_indexer_relays` instead of marking the author
    // `unroutable`. Same flag that `active_mute_list_interest` sets.
    interest.is_indexer_discovery = true;
    interest
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_planner::{InterestId, InterestLifecycle, InterestScope};

    // ── bookmark tests ────────────────────────────────────────────────────

    #[test]
    fn bookmark_interest_id_is_pubkey_invariant() {
        let id = active_bookmark_list_interest_id();
        assert_eq!(id, active_bookmark_list_interest_id());
        let _: fn() -> InterestId = active_bookmark_list_interest_id;
    }

    #[test]
    fn bookmark_interest_shape_matches_case_a_routing_contract() {
        let pk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let interest = active_bookmark_list_interest(pk);

        assert!(
            matches!(interest.lifecycle, InterestLifecycle::Tailing),
            "lifecycle must be Tailing; got {:?}",
            interest.lifecycle
        );
        assert!(
            matches!(interest.scope, InterestScope::Global),
            "scope must be Global; got {:?}",
            interest.scope
        );
        assert_eq!(
            interest.shape.kinds,
            std::collections::BTreeSet::from([KIND_BOOKMARK_LIST]),
            "shape.kinds must be EXACTLY {{kind:10003}}; got {:?}",
            interest.shape.kinds
        );
        assert_eq!(
            interest.shape.authors,
            std::collections::BTreeSet::from([pk.to_string()]),
            "shape.authors must be EXACTLY {{active_pubkey}}; got {:?}",
            interest.shape.authors
        );
        assert_eq!(interest.id, active_bookmark_list_interest_id());
    }

    // ── mute tests ────────────────────────────────────────────────────────

    #[test]
    fn mute_interest_id_is_pubkey_invariant() {
        let id = active_mute_list_interest_id();
        assert_eq!(id, active_mute_list_interest_id());
        let _: fn() -> InterestId = active_mute_list_interest_id;
    }

    #[test]
    fn mute_interest_shape_matches_case_a_authors_routing_contract() {
        let pk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let interest = active_mute_list_interest(pk);

        assert!(
            matches!(interest.lifecycle, InterestLifecycle::Tailing),
            "lifecycle must be Tailing; got {:?}",
            interest.lifecycle
        );
        assert!(
            matches!(interest.scope, InterestScope::Global),
            "scope must be Global; got {:?}",
            interest.scope
        );
        assert!(
            matches!(interest.shape.p_tag_routing, PTagRouting::Nip65ReadRelays),
            "p_tag_routing must be Nip65ReadRelays; got {:?}",
            interest.shape.p_tag_routing
        );
        // Exact-shape: a future over-broad kinds set (e.g. adding kind:3) must
        // fail here — mirrors the bookmark interest test's assert_eq pattern.
        assert_eq!(
            interest.shape.kinds,
            std::collections::BTreeSet::from([KIND_MUTE_LIST]),
            "shape.kinds must be EXACTLY {{kind:10000}}; got {:?}",
            interest.shape.kinds
        );
        // Exact-shape: authors must be exactly the one active pubkey passed in.
        assert_eq!(
            interest.shape.authors,
            std::collections::BTreeSet::from([pk.to_string()]),
            "shape.authors must be EXACTLY {{active_pubkey}}; got {:?}",
            interest.shape.authors
        );
        assert_eq!(interest.id, active_mute_list_interest_id());
        assert!(
            interest.is_indexer_discovery,
            "is_indexer_discovery must be true to enable the Case A \
             bootstrap_indexer_relays fallback on cold start; got false"
        );
    }

    // ── search-relay tests ────────────────────────────────────────────────

    #[test]
    fn search_relay_interest_id_is_pubkey_invariant() {
        let id = active_search_relay_list_interest_id();
        assert_eq!(id, active_search_relay_list_interest_id());
        let _: fn() -> InterestId = active_search_relay_list_interest_id;
    }

    #[test]
    fn search_relay_interest_shape_matches_case_a_routing_contract() {
        let pk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let interest = active_search_relay_list_interest(pk);

        assert!(
            matches!(interest.lifecycle, InterestLifecycle::Tailing),
            "lifecycle must be Tailing; got {:?}",
            interest.lifecycle
        );
        assert!(
            matches!(interest.scope, InterestScope::Global),
            "scope must be Global; got {:?}",
            interest.scope
        );
        assert!(
            matches!(interest.shape.p_tag_routing, PTagRouting::Nip65ReadRelays),
            "p_tag_routing must be Nip65ReadRelays; got {:?}",
            interest.shape.p_tag_routing
        );
        assert_eq!(
            interest.shape.kinds,
            std::collections::BTreeSet::from([KIND_SEARCH_RELAYS]),
            "shape.kinds must be EXACTLY {{kind:10007}}; got {:?}",
            interest.shape.kinds
        );
        assert_eq!(
            interest.shape.authors,
            std::collections::BTreeSet::from([pk.to_string()]),
            "shape.authors must be EXACTLY {{active_pubkey}}; got {:?}",
            interest.shape.authors
        );
        assert_eq!(interest.id, active_search_relay_list_interest_id());
        assert!(
            interest.is_indexer_discovery,
            "is_indexer_discovery must be true for the Case A cold-start \
             bootstrap_indexer_relays fallback; got false"
        );
    }
}
