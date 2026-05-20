//! Helpers for constructing Marmot `LogicalInterest`s.
//!
//! Per `docs/plan/marmot-mls.md` §Step 4 + mdk-api.md §4, every relay read the
//! Marmot app needs is represented as a kernel interest:
//!
//! - kind:1059 `#p = self` gift-wrap inbox, registered at Marmot startup;
//! - kind:30443/443 KeyPackage lookup, registered when an invite flow needs a
//!   peer's package; and
//! - relay-pinned kind:445 group messages, registered when the group relays are
//!   known from group creation or a Welcome.
//!
//! The raw-event tap then drives accepted signed events into `MarmotService`.

use std::collections::BTreeSet;

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

/// Stable id for a peer KeyPackage lookup subscription.
fn key_package_lookup_interest_id(pubkey: &str) -> InterestId {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "marmot.key_package_lookup".hash(&mut h);
    pubkey.hash(&mut h);
    InterestId(h.finish())
}

/// Stable id for one relay-pinned group-message subscription.
fn group_message_interest_id(group_id_hex: &str, relay_url: &str) -> InterestId {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "marmot.group_messages".hash(&mut h);
    group_id_hex.hash(&mut h);
    relay_url.hash(&mut h);
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
    let deps = nmp_core::substrate::ViewDependencies {
        kinds: vec![KIND_GIFT_WRAP],
        tag_refs: vec![("p".to_string(), pubkey.to_string())],
        ..Default::default()
    };
    deps.into_logical_interest(
        giftwrap_interest_id(pubkey),
        nmp_core::planner::InterestScope::Account(pubkey.to_string()),
        InterestLifecycle::Tailing,
    )
}

/// Tailing author-scoped KeyPackage lookup for invite flows.
///
/// KeyPackage events are addressable replaceable events published to the
/// author's outbox relays. The kernel planner owns that NIP-65 routing; the
/// app only declares the peer pubkey and the event kinds it needs.
pub fn key_package_lookup_interest(pubkey: &str) -> LogicalInterest {
    let mut authors = BTreeSet::new();
    authors.insert(pubkey.to_string());
    LogicalInterest {
        id: key_package_lookup_interest_id(pubkey),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors,
            kinds: [KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY]
                .into_iter()
                .collect(),
            limit: Some(4),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    }
}

/// Relay-pinned tailing interests for group kind:445 traffic.
///
/// Marmot group traffic is bound to the group relays, not author outboxes. Each
/// relay gets its own hard-pinned interest so the kernel keeps the corresponding
/// REQ open and the raw-event tap receives messages without an inbox sweep.
pub fn group_message_interests(
    group_id_hex: &str,
    relays: impl IntoIterator<Item = String>,
) -> Vec<LogicalInterest> {
    relays
        .into_iter()
        .map(|relay_url| LogicalInterest {
            id: group_message_interest_id(group_id_hex, &relay_url),
            scope: InterestScope::Global,
            shape: InterestShape {
                kinds: [KIND_GROUP_MESSAGE].into_iter().collect(),
                limit: Some(200),
                relay_pin: Some(relay_url.clone()),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        })
        .collect()
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

    #[test]
    fn key_package_lookup_interest_targets_peer_package_kinds() {
        let i = key_package_lookup_interest("peerpubkey");
        assert!(i.shape.authors.contains("peerpubkey"));
        assert!(i.shape.kinds.contains(&KIND_KEY_PACKAGE));
        assert!(i.shape.kinds.contains(&KIND_KEY_PACKAGE_LEGACY));
        assert_eq!(i.shape.limit, Some(4));
        assert!(i.shape.relay_pin.is_none());
        assert!(matches!(i.lifecycle, InterestLifecycle::Tailing));
        assert_eq!(i.id, key_package_lookup_interest_id("peerpubkey"));
    }

    #[test]
    fn group_message_interests_are_relay_pinned_and_tailing() {
        let interests = group_message_interests(
            "abcd",
            ["wss://group-a/", "wss://group-b/"]
                .into_iter()
                .map(String::from),
        );
        assert_eq!(interests.len(), 2);
        for i in &interests {
            assert!(i.shape.kinds.contains(&KIND_GROUP_MESSAGE));
            assert_eq!(i.shape.limit, Some(200));
            assert!(matches!(i.lifecycle, InterestLifecycle::Tailing));
            assert!(matches!(i.scope, InterestScope::Global));
        }
        assert_eq!(
            interests[0].shape.relay_pin.as_deref(),
            Some("wss://group-a/")
        );
        assert_eq!(
            interests[1].shape.relay_pin.as_deref(),
            Some("wss://group-b/")
        );
        assert_ne!(interests[0].id, interests[1].id);
    }
}
