//! NIP-51 active-account bookmark-list subscription interest.
//!
//! The host-driven counterpart to `nmp_nip57::self_zap_receipts_interest`
//! for bookmark lists (kind:10003) authored by the active account. A host shell
//! wires this through a runtime controller (see
//! `crates/nmp-defaults/src/runtimes/bookmarks_runtime.rs`) so the kernel
//! learns nothing about NIP-51 bookmarks — it just routes a generic
//! [`LogicalInterest`] exactly the way it routes any other interest.
//!
//! # Why `Global + Nip65ReadRelays`
//!
//! Kind:10003 bookmark lists are public replaceable events that live on the
//! author's content / read-relay set (kind:10002), so they use
//! `PTagRouting::Nip65ReadRelays` and `InterestScope::Global`. The `Global`
//! scope is load-bearing: it lets the planner's cold-start fallback at
//! `crates/nmp-core/src/planner/compiler/partition/mod.rs` fire when no
//! kind:10002 has arrived yet — the gate evaluates
//! `lifecycle == Tailing && scope == Global && authors=[pubkey]`
//! and routes the interest to `bootstrap_content_relays` until the real
//! NIP-65 read inbox is cached.
//!
//! # Single-slot semantics
//!
//! [`active_bookmark_list_interest_id`] is pubkey-invariant on purpose: the
//! controller withdraws the prior interest by id and pushes a fresh one on
//! account switch, so the kernel never accumulates one standing subscription
//! per ever-active pubkey. Mirrors the NIP-57 zap-receipts slot pattern.

use nmp_planner::{
    InterestId, InterestLifecycle, InterestScope, LogicalInterest, PTagRouting,
};
use nmp_core::substrate::ViewDependencies;

use nmp_kinds::KIND_BOOKMARK_LIST;

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
/// Shape — read by the planner's cold-start bootstrap gate at
/// `crates/nmp-core/src/planner/compiler/partition/mod.rs`:
/// - `lifecycle = Tailing`
/// - `scope = Global`
/// - `kinds = [10003]`
/// - `authors = [pubkey]`
/// - `p_tag_routing = Nip65ReadRelays`
///
/// When the active account has no cached NIP-65 inbox yet (cold start), the
/// planner routes this interest to `bootstrap_content_relays` so bookmark
/// events keep flowing until the real read-relay set lands. Once kind:10002
/// arrives, the next recompile re-routes to the real inbox + emits the
/// matching CLOSE on the bootstrap landing.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The interest id is pubkey-invariant — locks the no-arg signature
    /// against a future refactor that adds a pubkey parameter (which would
    /// break the single-slot withdraw/re-push contract the runtime controller
    /// relies on for account switch).
    #[test]
    fn interest_id_is_pubkey_invariant() {
        let id = active_bookmark_list_interest_id();
        // Calling again yields the same id (id is a constant hash of a
        // fixed string).
        assert_eq!(id, active_bookmark_list_interest_id());
        // The signature takes no pubkey arg, so the id literally cannot vary
        // with pubkey. The structural assertion is the test contract.
        // Asserting the symbol exists, takes no args, and returns InterestId
        // — locks all three against a refactor.
        let _: fn() -> InterestId = active_bookmark_list_interest_id;
    }

    /// The interest shape matches the planner cold-start bootstrap gate
    /// (`partition/mod.rs`: Tailing + Global + authors=[pubkey]). Without
    /// this exact shape, the cold-start fallback would not fire and
    /// `BookmarkListProjection` would receive no bookmark events until
    /// kind:10002 arrives for the active account.
    #[test]
    fn interest_shape_matches_planner_bootstrap_gate() {
        let pk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let interest = active_bookmark_list_interest(pk);

        assert!(
            matches!(interest.lifecycle, InterestLifecycle::Tailing),
            "lifecycle must be Tailing — the planner gate keys on this; got {:?}",
            interest.lifecycle
        );
        assert!(
            matches!(interest.scope, InterestScope::Global),
            "scope must be Global — bookmark lists are public content on \
             the author's read relays, NOT private DM relays; got {:?}",
            interest.scope
        );
        assert!(
            interest.shape.kinds.contains(&KIND_BOOKMARK_LIST),
            "shape.kinds must include kind:10003; got {:?}",
            interest.shape.kinds
        );
        assert!(
            interest.shape.authors.contains(&pk.to_string()),
            "shape.authors must contain the active account pubkey; got {:?}",
            interest.shape.authors
        );
        // The id matches the pubkey-invariant slot id — withdraws by id work.
        assert_eq!(interest.id, active_bookmark_list_interest_id());
    }
}
