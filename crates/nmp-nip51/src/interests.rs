//! NIP-51 active-account mute-list subscription interest.
//!
//! The host-driven counterpart to `nmp_nip57::self_zap_receipts_interest`
//! for mute lists (kind:10000) authored by the active account. A host shell
//! wires this through a `MuteRuntimeController` (see
//! `crates/nmp-defaults/src/runtimes/mute_runtime.rs`) so the kernel
//! learns nothing about NIP-51 mutes — it just routes a generic
//! [`LogicalInterest`] exactly the way it routes any other interest.
//!
//! # Why `Global + Nip65ReadRelays`
//!
//! Kind:10000 mute lists are public replaceable events authored by the
//! active account, so the interest carries `authors=[pubkey]` and routes
//! through the planner's **Case A** (explicit authors → outbox / write relays).
//! `InterestScope::Global` ensures the full relay-lane set is evaluated;
//! `PTagRouting::Nip65ReadRelays` prevents fail-closed DM-relay routing.
//!
//! # Single-slot semantics
//!
//! [`active_mute_list_interest_id`] is pubkey-invariant on purpose: the
//! controller withdraws the prior interest by id and pushes a fresh one on
//! account switch. Mirrors the NIP-57 zap-receipts slot pattern.

use nmp_core::substrate::ViewDependencies;
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest, PTagRouting};

use nmp_kinds::KIND_MUTE_LIST;

/// Stable id for the active-account-owned mute-list interest.
#[must_use]
pub fn active_mute_list_interest_id() -> InterestId {
    InterestId(nmp_planner::stable_hash::stable_hash64(
        "nmp.nip51.active_mute_list",
    ))
}

/// Tailing [`LogicalInterest`] for kind:10000 `authors=[pubkey]` mute lists.
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
    interest
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_planner::{InterestId, InterestLifecycle, InterestScope};

    /// The interest id is pubkey-invariant — locks the no-arg signature
    /// against a future refactor that adds a pubkey parameter (which would
    /// break the single-slot withdraw/re-push contract the runtime controller
    /// relies on for account switch).
    #[test]
    fn interest_id_is_pubkey_invariant() {
        let id = active_mute_list_interest_id();
        // Calling again yields the same id (id is a constant hash of a
        // fixed string).
        assert_eq!(id, active_mute_list_interest_id());
        // The signature takes no pubkey arg, so the id literally cannot vary
        // with pubkey. The structural assertion is the test contract.
        let _: fn() -> InterestId = active_mute_list_interest_id;
    }

    /// The interest shape matches the planner Case A authors-routing contract:
    /// authors=[pubkey], kinds=[10000], lifecycle=Tailing, scope=Global,
    /// p_tag_routing=Nip65ReadRelays.
    #[test]
    fn interest_shape_matches_case_a_authors_routing_contract() {
        let pk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let interest = active_mute_list_interest(pk);

        assert!(
            matches!(interest.lifecycle, InterestLifecycle::Tailing),
            "lifecycle must be Tailing; got {:?}",
            interest.lifecycle
        );
        assert!(
            matches!(interest.scope, InterestScope::Global),
            "scope must be Global — mute lists are public author-keyed events \
             on outbox relays, not private DM-relay events; got {:?}",
            interest.scope
        );
        assert!(
            matches!(interest.shape.p_tag_routing, PTagRouting::Nip65ReadRelays),
            "p_tag_routing must be Nip65ReadRelays to prevent fail-closed \
             DM-relay routing; got {:?}",
            interest.shape.p_tag_routing
        );
        assert!(
            interest.shape.kinds.contains(&KIND_MUTE_LIST),
            "shape.kinds must include kind:10000; got {:?}",
            interest.shape.kinds
        );
        // Case A: mute lists are author-keyed, NOT #p-tagged. Routing happens
        // through shape.authors, not shape.tags["p"].
        assert!(
            interest.shape.authors.contains(pk),
            "shape.authors must contain the active account pubkey; got {:?}",
            interest.shape.authors
        );
        // The id matches the pubkey-invariant slot id — withdraws by id work.
        assert_eq!(interest.id, active_mute_list_interest_id());
    }
}
