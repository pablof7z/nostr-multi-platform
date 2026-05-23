//! NIP-57 self-zap-receipts subscription interest.
//!
//! The host-driven counterpart to `nmp_nip17::active_giftwrap_inbox_interest`
//! for zap receipts (kind:9735) addressed to the active account. A host shell
//! wires this through a runtime controller (see
//! `apps/chirp/nmp-app-chirp/src/zap_receipts_runtime.rs` for the canonical
//! reference) so the kernel learns nothing about NIP-57 — it just routes a
//! generic `LogicalInterest` exactly the way it routes a NIP-17 gift-wrap
//! inbox interest.
//!
//! # Why `Global + Nip65ReadRelays` (NOT `ActiveAccount + Nip17DmRelays`)
//!
//! NIP-17 gift-wraps live on the recipient's *DM-relay* set (kind:10050) and
//! MUST stay fail-closed when that set is unknown — a gift-wrap leaking to a
//! non-DM relay is a privacy regression. Zap receipts (NIP-57 § "Appendix F")
//! are public events that live on the recipient's *content / read-relay* set
//! (kind:10002), so they use `PTagRouting::Nip65ReadRelays`.
//!
//! `InterestScope::Global` is the load-bearing knob: it lets the planner's
//! cold-start fallback at
//! `crates/nmp-core/src/planner/compiler/partition/mod.rs` fire when no
//! kind:10002 has arrived yet — the gate evaluates
//! `lifecycle == Tailing && scope == Global && p_tag_routing == Nip65ReadRelays`
//! and routes the interest to `bootstrap_content_relays` until the real
//! NIP-65 read inbox is cached. `InterestScope::ActiveAccount` (the NIP-17
//! choice) intentionally bypasses that gate because gift-wraps must NEVER
//! divert to a bootstrap relay.
//!
//! # Single-slot semantics
//!
//! [`self_zap_receipts_interest_id`] is pubkey-invariant on purpose: the
//! controller withdraws the prior interest by id and pushes a fresh one on
//! account switch, so the kernel never accumulates one standing subscription
//! per ever-active pubkey. Mirrors the NIP-17 inbox slot.

use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, LogicalInterest, PTagRouting,
};
use nmp_core::substrate::ViewDependencies;

use crate::kinds::KIND_ZAP_RECEIPT;

/// Stable id for the active-account-owned self-zap-receipts interest.
///
/// The id is intentionally independent of the pubkey so an account switch
/// replaces the prior `#p` filter instead of accumulating one long-lived
/// subscription per account. Mirrors
/// [`nmp_nip17::active_giftwrap_inbox_interest_id`] line for line.
#[must_use]
pub fn self_zap_receipts_interest_id() -> InterestId {
    InterestId(nmp_core::stable_hash::stable_hash64(
        "nip57.zap_receipts.active",
    ))
}

/// Tailing [`LogicalInterest`] for kind:9735 `#p <pubkey>` zap receipts — the
/// subscription a host pushes (via `NmpApp::push_interest` / a runtime
/// controller) so a `ZapsAggregateProjection` actually receives receipts.
///
/// Shape — read by the planner's cold-start bootstrap gate at
/// `crates/nmp-core/src/planner/compiler/partition/mod.rs`:
/// - `lifecycle = Tailing`
/// - `scope = Global`
/// - `kinds = [9735]`
/// - `#p = [pubkey]`
/// - `p_tag_routing = Nip65ReadRelays`
///
/// When the active account has no cached NIP-65 inbox yet (cold start), the
/// planner routes this interest to `bootstrap_content_relays` so receipts
/// keep flowing until the real read-relay set lands. Once kind:10002
/// arrives, the next recompile re-routes to the real inbox + emits the
/// matching CLOSE on the bootstrap landing.
#[must_use]
pub fn self_zap_receipts_interest(pubkey: &str) -> LogicalInterest {
    let deps = ViewDependencies {
        kinds: vec![KIND_ZAP_RECEIPT],
        tag_refs: vec![("p".to_string(), pubkey.to_string())],
        ..Default::default()
    };
    let mut interest = deps.into_logical_interest(
        self_zap_receipts_interest_id(),
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
        let id = self_zap_receipts_interest_id();
        // Calling again yields the same id (id is a constant hash of a
        // fixed string).
        assert_eq!(id, self_zap_receipts_interest_id());
        // The signature takes no pubkey arg, so the id literally cannot vary
        // with pubkey. The structural assertion is the test contract.
        // Asserting the symbol exists, takes no args, and returns InterestId
        // — locks all three against a refactor.
        let _: fn() -> InterestId = self_zap_receipts_interest_id;
    }

    /// The interest shape matches the planner cold-start bootstrap gate
    /// (`partition/mod.rs`: Tailing + Global + #p + Nip65ReadRelays). Without
    /// this exact shape, the cold-start fallback would not fire and
    /// `ZapsAggregateProjection` would receive no receipts until kind:10002
    /// arrives for the active account.
    #[test]
    fn interest_shape_matches_planner_bootstrap_gate() {
        let pk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let interest = self_zap_receipts_interest(pk);

        assert!(
            matches!(interest.lifecycle, InterestLifecycle::Tailing),
            "lifecycle must be Tailing — the planner gate keys on this; got {:?}",
            interest.lifecycle
        );
        assert!(
            matches!(interest.scope, InterestScope::Global),
            "scope must be Global — Nip65 read-relays is public content, NOT \
             ActiveAccount (which is the NIP-17 DM-relays choice for private \
             gift-wraps); got {:?}",
            interest.scope
        );
        assert!(
            matches!(interest.shape.p_tag_routing, PTagRouting::Nip65ReadRelays),
            "p_tag_routing must be Nip65ReadRelays — zap receipts are public \
             content on the recipient's read inbox, NOT private DM relays; got \
             {:?}",
            interest.shape.p_tag_routing
        );
        assert!(
            interest.shape.kinds.contains(&KIND_ZAP_RECEIPT),
            "shape.kinds must include kind:9735; got {:?}",
            interest.shape.kinds
        );
        // `tags` is a `BTreeMap<String, BTreeSet<String>>` keyed by tag name
        // — the planner's `#p` lookup reads the value set for the `"p"` entry.
        // Assert the structural shape rather than a flat iterator: an absent
        // `"p"` key, or a value-set missing the pubkey, are both regressions
        // the planner cannot route around.
        let p_values = interest
            .shape
            .tags
            .get("p")
            .cloned()
            .unwrap_or_default();
        assert!(
            p_values.contains(pk),
            "shape.tags[\"p\"] must contain the active account pubkey; got {:?}",
            interest.shape.tags
        );
        // The id matches the pubkey-invariant slot id — withdraws by id work.
        assert_eq!(interest.id, self_zap_receipts_interest_id());
    }
}
