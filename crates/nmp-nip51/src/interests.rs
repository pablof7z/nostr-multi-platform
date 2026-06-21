//! NIP-51 active-account mute-list subscription interest.
//!
//! The host-driven counterpart to `nmp_nip57::self_zap_receipts_interest`
//! for mute lists (kind:10000) authored by the active account. A host shell
//! wires this through a `MuteRuntimeController` (see
//! `crates/nmp-defaults/src/runtimes/mute_runtime.rs`) so the kernel
//! learns nothing about NIP-51 mutes — it just routes a generic
//! [`LogicalInterest`] exactly the way it routes any other interest.
//!
//! # Why `Global + is_indexer_discovery: true`
//!
//! Kind:10000 mute lists are public replaceable events authored by the
//! active account, so the interest carries `authors=[pubkey]` and routes
//! through the planner's **Case A** (explicit authors → outbox / write relays).
//! `InterestScope::Global` ensures the full relay-lane set is evaluated;
//! `PTagRouting::Nip65ReadRelays` prevents fail-closed DM-relay routing.
//!
//! `is_indexer_discovery: true` opts the interest into the
//! `case_a_authors` bootstrap fallback: when the active account's NIP-65
//! mailbox is unknown AND no `app_relays` are configured (the cold-start
//! chicken-and-egg — NIP-65 itself hasn't landed yet), the planner routes
//! the interest to `bootstrap_indexer_relays` rather than marking the
//! author `unroutable`. This matches the behaviour of `SELF_KINDS_TAILING`
//! in `nmp-core/src/kernel/requests/startup.rs` which this interest replaces
//! (kind:10000 was removed from that constant — this flag preserves the same
//! cold-start guarantee).
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
///
/// Shape read by the planner's Case A routing (explicit authors → outbox):
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
        // `is_indexer_discovery` must be true — Case A cold-start fallback.
        // Without this, a fresh install with no NIP-65 mailbox and no app_relays
        // would mark the author `unroutable` and kind:10000 would never arrive.
        assert!(
            interest.is_indexer_discovery,
            "is_indexer_discovery must be true to enable the Case A \
             bootstrap_indexer_relays fallback on cold start (mirrors \
             SELF_KINDS_TAILING's is_indexer_discovery flag); got false"
        );
    }
}
