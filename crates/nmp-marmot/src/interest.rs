//! Helpers for constructing Marmot `LogicalInterest`s.
//!
//! Per `docs/plan/marmot-mls.md` §Step 4 + mdk-api.md §4, the only interest
//! the app layer pushes today is the inbound Welcome gift-wrap subscription
//! ([`giftwrap_inbox_interest`]): kind:1059 `#p = self`, `Account`-scoped,
//! routed to the recipient's NIP-65 inbox relays (NO relay pin). The kernel
//! raw-event tap then drives accepted events into `MarmotService`.
//!
//! The KeyPackage event kinds (30443/443) and group-message kind (445) are
//! exported as constants for the dispatch / publish-plan layers; the per-kind
//! interest builders for them are not constructed yet (the projection drives
//! key-package fetch + group polling directly via `poll_inbox`), so no
//! speculative builders are kept here (Article VII — no future-proofing).

use std::collections::BTreeMap;

use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};

/// Marmot KeyPackage event kind (NIP-33 addressable). CURRENT spec.
pub const KIND_KEY_PACKAGE: u32 = 30443;
/// Marmot KeyPackage legacy kind. Dual-published through 2026-05-31.
pub const KIND_KEY_PACKAGE_LEGACY: u32 = 443;
/// Marmot group message / commit / proposal (MLS + MIP-03 outer layer).
pub const KIND_GROUP_MESSAGE: u32 = 445;
/// NIP-59 gift-wrap kind (carries the kind:444 Welcome rumor).
pub const KIND_GIFT_WRAP: u32 = 1059;

/// Stable, deterministic `InterestId` for a pubkey's gift-wrap inbox
/// subscription. Same hash pattern as `follow_feed_interest_id` in the
/// kernel's contacts ingest — keying the id off the pubkey lets a per-app
/// FFI bridge push the interest idempotently (re-registration produces the
/// same id, the kernel de-dupes).
fn giftwrap_interest_id(pubkey: &str) -> InterestId {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "marmot.giftwrap".hash(&mut h);
    pubkey.hash(&mut h);
    InterestId(h.finish())
}

/// Tailing `LogicalInterest` for kind:1059 `#p <pubkey>` gift-wraps — the
/// event-driven Welcome-delivery subscription a per-app FFI bridge pushes at
/// Marmot registration time. This is the policy seam: kind selection, the
/// deterministic id, the `#p` filter and the `Account` scope are protocol
/// decisions and therefore live in `nmp-marmot`, not in any app's glue (D7).
///
/// Scope is [`InterestScope::Account`] (bound to the specific `pubkey`)
/// rather than `ActiveAccount`: the bridge resolves the concrete identity at
/// registration and the subscription must stay pinned to it. The kernel's
/// raw-event tap then drives every accepted event into
/// `MarmotService::ingest_signed_event_core` automatically.
pub fn giftwrap_inbox_interest(pubkey: &str) -> LogicalInterest {
    let mut tags = BTreeMap::new();
    tags.insert(
        "p".to_string(),
        [pubkey.to_string()].into_iter().collect(),
    );
    LogicalInterest {
        id: giftwrap_interest_id(pubkey),
        scope: InterestScope::Account(pubkey.to_string()),
        shape: InterestShape {
            kinds: [KIND_GIFT_WRAP].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn giftwrap_interest_id_is_deterministic_per_pubkey() {
        let a = giftwrap_interest_id("abc123");
        let b = giftwrap_interest_id("abc123");
        let c = giftwrap_interest_id("def456");
        assert_eq!(a, b, "same pubkey must yield same id");
        assert_ne!(a, c, "different pubkeys must yield different ids");
    }

    #[test]
    fn giftwrap_inbox_interest_is_account_scoped_and_p_filtered() {
        let i = giftwrap_inbox_interest("selfpubkey");
        assert!(i.shape.relay_pin.is_none());
        assert!(i.shape.kinds.contains(&KIND_GIFT_WRAP));
        assert!(i.shape.tags.get("p").unwrap().contains("selfpubkey"));
        assert!(matches!(i.lifecycle, InterestLifecycle::Tailing));
        assert!(matches!(
            i.scope,
            InterestScope::Account(ref pk) if pk == "selfpubkey"
        ));
        assert_eq!(i.id, giftwrap_interest_id("selfpubkey"));
    }
}
