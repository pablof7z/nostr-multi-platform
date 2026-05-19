//! Helpers for constructing Marmot `LogicalInterest`s.
//!
//! Per `docs/plan/marmot-mls.md` §Step 4 + mdk-api.md §4:
//! - Group events (kind:445) are relay-pinned to the group relay
//!   (`InterestShape::relay_pin = Some(group_relay)`) — ADR-0012 third lane,
//!   mirroring `nmp-nip29::interest`.
//! - KeyPackage events (kind:30443 + legacy kind:443) use standard
//!   author-write outbox routing — NO pin.
//! - Welcome gift-wraps (kind:1059, `#p` = self) route to the recipient's
//!   NIP-65 inbox relays — NO pin.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};

/// Marmot KeyPackage event kind (NIP-33 addressable). CURRENT spec.
pub const KIND_KEY_PACKAGE: u32 = 30443;
/// Marmot KeyPackage legacy kind. Dual-published through 2026-05-31.
pub const KIND_KEY_PACKAGE_LEGACY: u32 = 443;
/// MLS Welcome rumor kind (wrapped in NIP-59 kind:1059).
pub const KIND_WELCOME_RUMOR: u32 = 444;
/// Marmot group message / commit / proposal (MLS + MIP-03 outer layer).
pub const KIND_GROUP_MESSAGE: u32 = 445;
/// NIP-59 gift-wrap kind (carries the kind:444 Welcome rumor).
pub const KIND_GIFT_WRAP: u32 = 1059;

/// Tailing interest for a group's kind:445 stream, relay-pinned to the group
/// relay (ADR-0012 third lane / lattice Rule 9). All group commits, proposals
/// and application messages flow through this single per-group REQ.
pub fn group_messages_interest(
    id: u64,
    group_relay_url: &str,
    group_nostr_id_hex: &str,
) -> LogicalInterest {
    // kind:445 events carry the (rotating) nostr group id as an `h` tag; the
    // relay-side filter is the `h` tag, the client-side routing is the pin.
    let mut tags = BTreeMap::new();
    tags.insert(
        "h".to_string(),
        [group_nostr_id_hex.to_string()].into_iter().collect(),
    );
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            kinds: [KIND_GROUP_MESSAGE].into_iter().collect(),
            tags,
            relay_pin: Some(group_relay_url.to_string()),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    }
}

/// One-shot interest fetching a peer's published KeyPackage(s) for invitation.
/// Standard author-write outbox routing — NOT relay-pinned (the planner
/// resolves the author's NIP-65 write relays).
pub fn key_packages_for(id: u64, owner_pubkey: &str) -> LogicalInterest {
    let authors: BTreeSet<String> = [owner_pubkey.to_string()].into_iter().collect();
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors,
            kinds: [KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY]
                .into_iter()
                .collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
    }
}

/// Tailing interest for inbound Welcome gift-wraps addressed to `self`.
/// Routes to the recipient's NIP-65 inbox relays — NOT relay-pinned.
///
/// See also [`giftwrap_inbox_interest`]: a near-twin that takes a
/// caller-supplied `pubkey`, derives a deterministic id, and pins the scope
/// to that concrete `Account` rather than `ActiveAccount`. They are kept
/// separate deliberately — different callers, different scope invariants.
pub fn welcomes_for(id: u64, self_pubkey: &str) -> LogicalInterest {
    let mut tags = BTreeMap::new();
    tags.insert(
        "p".to_string(),
        [self_pubkey.to_string()].into_iter().collect(),
    );
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            kinds: [KIND_GIFT_WRAP].into_iter().collect(),
            tags,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    }
}

/// Stable, deterministic `InterestId` for a pubkey's gift-wrap inbox
/// subscription. Same hash pattern as `follow_feed_interest_id` in the
/// kernel's contacts ingest — keying the id off the pubkey lets a per-app
/// FFI bridge push the interest idempotently (re-registration produces the
/// same id, the kernel de-dupes).
pub fn giftwrap_interest_id(pubkey: &str) -> InterestId {
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
///
/// See also [`welcomes_for`]: the same kind:1059 `#p` filter but
/// `ActiveAccount`-scoped with a caller-supplied id. Kept separate on
/// purpose — do not merge.
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
    fn group_messages_pins_to_group_relay() {
        let i = group_messages_interest(1, "wss://group.example.com", "deadbeef");
        assert_eq!(
            i.shape.relay_pin.as_deref(),
            Some("wss://group.example.com")
        );
        assert!(i.shape.kinds.contains(&KIND_GROUP_MESSAGE));
        assert!(i.shape.tags.get("h").unwrap().contains("deadbeef"));
    }

    #[test]
    fn key_packages_are_not_pinned() {
        let i = key_packages_for(2, "peerpubkey");
        assert!(i.shape.relay_pin.is_none());
        assert!(i.shape.kinds.contains(&KIND_KEY_PACKAGE));
        assert!(i.shape.kinds.contains(&KIND_KEY_PACKAGE_LEGACY));
        assert!(i.shape.authors.contains("peerpubkey"));
    }

    #[test]
    fn welcomes_are_not_pinned_and_p_filtered() {
        let i = welcomes_for(3, "selfpubkey");
        assert!(i.shape.relay_pin.is_none());
        assert!(i.shape.kinds.contains(&KIND_GIFT_WRAP));
        assert!(i.shape.tags.get("p").unwrap().contains("selfpubkey"));
    }

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
