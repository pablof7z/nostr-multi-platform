//! Helpers for constructing Marmot `LogicalInterest`s.
//!
//! Per `docs/research/mdk-api.md` §4, every relay read the
//! Marmot app needs is represented as a kernel interest:
//!
//! - kind:1059 `#p = self` gift-wrap inbox, registered at Marmot startup;
//! - kind:30443 KeyPackage lookup, registered when an invite flow needs a
//!   peer's package (legacy kind:443 retired 2026-05-31); and
//! - relay-pinned kind:445 group messages, registered when the group relays are
//!   known from group creation or a Welcome.
//!
//! Marmot's ingest parser then drives accepted signed events into
//! `MarmotService`.

use nmp_core::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use nmp_planner::stable_hash::stable_hash64;
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest};
// Kind integers from the canonical Layer-0 registry (`nmp-kinds`, reached via
// nmp-nip59 / nmp-core). KIND_GIFT_WRAP = 1059; the Marmot key-package,
// group-message, and welcome kinds were previously re-declared as literals in
// several places (`interest.rs`, `service.rs` as u16, `projection/state.rs`,
// `tap.rs`) — a u16/u32 type split flagged by the #1493 fragmentation audit.
// They are now ONE canonical `u32` definition re-exported here for the rest of
// the crate (these constants have no external importers). NOTE: the `ops.rs`
// dispatch still matches on `event.kind.as_u16()` literals — left as-is to keep
// that 500-LOC god-file from expanding over its file-size baseline.
//
// KIND_MARMOT_KEY_PACKAGE_LEGACY (kind:443) is intentionally NOT re-exported:
// the legacy dual-publish was retired 2026-05-31; nmp-marmot now only
// publishes/subscribes kind:30443.
pub use nmp_core::kinds::{
    KIND_MARMOT_GROUP_MESSAGE, KIND_MARMOT_KEY_PACKAGE, KIND_MARMOT_WELCOME,
};
pub use nmp_nip59::KIND_GIFT_WRAP;

/// Stable, deterministic `InterestId` for a pubkey's gift-wrap inbox
/// subscription. Same hash pattern as `follow_feed_interest_id` in the
/// kernel's contacts ingest — keying the id off the pubkey lets a per-app
/// host registration layer push the interest idempotently (re-registration produces the
/// same id, the kernel de-dupes).
fn giftwrap_interest_id(pubkey: &str) -> InterestId {
    InterestId(stable_hash64(("marmot.giftwrap", pubkey)))
}

/// Stable id for a peer KeyPackage lookup subscription.
fn key_package_lookup_interest_id(pubkey: &str) -> InterestId {
    InterestId(stable_hash64(("marmot.key_package_lookup", pubkey)))
}

/// Stable id for one relay-pinned group-message subscription.
fn group_message_interest_id(group_id_hex: &str, relay_url: &str) -> InterestId {
    InterestId(stable_hash64((
        "marmot.group_messages",
        group_id_hex,
        relay_url,
    )))
}

/// Tailing `LogicalInterest` for kind:1059 `#p <pubkey>` gift-wraps — the
/// event-driven Welcome-delivery subscription a host registration layer pushes at
/// Marmot registration time. This is the policy seam: kind selection, the
/// deterministic id, the `#p` filter and the `Account` scope are protocol
/// decisions and therefore live in `nmp-marmot`, not in any app's glue (D7).
///
/// Scope is [`InterestScope::Account`] (bound to the specific `pubkey`)
/// rather than `ActiveAccount`: the bridge resolves the concrete identity at
/// registration and the subscription must stay pinned to it. The kernel's
/// Marmot's ingest parser then drives every accepted event into
/// `MarmotService::ingest_signed_event_core` automatically.
#[must_use]
pub fn giftwrap_inbox_interest(pubkey: &str) -> LogicalInterest {
    let deps = nmp_core::substrate::ViewDependencies {
        kinds: vec![KIND_GIFT_WRAP],
        tag_refs: vec![("p".to_string(), pubkey.to_string())],
        ..Default::default()
    };
    deps.into_logical_interest(
        giftwrap_interest_id(pubkey),
        nmp_planner::InterestScope::Account(pubkey.to_string()),
        InterestLifecycle::Tailing,
    )
}

/// Scoped registry identity for a Marmot gift-wrap inbox subscription.
#[must_use]
pub fn giftwrap_inbox_identity(pubkey: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(("marmot.giftwrap", pubkey)),
        SubKey::new(("marmot.giftwrap", pubkey)),
        SubScope::Account(pubkey.to_string()),
    )
}

/// Tailing author-scoped KeyPackage lookup for invite flows.
///
/// KeyPackage events are addressable replaceable events published to the
/// author's outbox relays. The kernel planner owns that NIP-65 routing; the
/// app only declares the peer pubkey and the event kind it needs.
///
/// Only kind:30443 is subscribed (legacy kind:443 was retired 2026-05-31).
#[must_use]
pub fn key_package_lookup_interest(pubkey: &str) -> LogicalInterest {
    nmp_core::substrate::ViewDependencies {
        kinds: vec![KIND_MARMOT_KEY_PACKAGE],
        authors: vec![pubkey.to_string()],
        limit: Some(4),
        ..Default::default()
    }
    .into_logical_interest(
        key_package_lookup_interest_id(pubkey),
        InterestScope::Global,
        InterestLifecycle::Tailing,
    )
}

/// Scoped registry identity for a peer KeyPackage lookup subscription.
#[must_use]
pub fn key_package_lookup_identity(pubkey: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(("marmot.key_package_lookup", pubkey)),
        SubKey::new(("marmot.key_package_lookup", pubkey)),
        SubScope::Global,
    )
}

/// Scoped registry identity for one relay-pinned group-message subscription.
#[must_use]
pub fn group_message_identity(group_id_hex: &str, relay_url: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(("marmot.group_messages", group_id_hex, relay_url)),
        SubKey::new(("marmot.group_messages", group_id_hex, relay_url)),
        SubScope::Global,
    )
}

/// Relay-pinned tailing interests for group kind:445 traffic.
///
/// Marmot group traffic is bound to the group relays, not author outboxes. Each
/// relay gets its own hard-pinned interest so the kernel keeps the corresponding
/// REQ open and Marmot's ingest parser receives messages without an inbox
/// sweep.
///
pub fn group_message_interests(
    group_id_hex: &str,
    relays: impl IntoIterator<Item = String>,
) -> Vec<LogicalInterest> {
    group_message_registrations(group_id_hex, relays)
        .into_iter()
        .map(|(_, interest)| interest)
        .collect()
}

/// Relay-pinned group subscriptions paired with their scoped identities.
pub fn group_message_registrations(
    group_id_hex: &str,
    relays: impl IntoIterator<Item = String>,
) -> Vec<(SubIdentity, LogicalInterest)> {
    relays
        .into_iter()
        .map(|relay_url| {
            let identity = group_message_identity(group_id_hex, &relay_url);
            let interest = nmp_core::substrate::ViewDependencies {
                kinds: vec![KIND_MARMOT_GROUP_MESSAGE],
                relay_pin: Some(relay_url.clone()),
                limit: Some(200),
                ..Default::default()
            }
            .into_logical_interest(
                group_message_interest_id(group_id_hex, &relay_url),
                InterestScope::Global,
                InterestLifecycle::Tailing,
            );
            (identity, interest)
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
        assert_eq!(a, InterestId(0x95ff_bdc5_c509_4315));
    }

    #[test]
    fn lookup_and_group_interest_ids_are_restart_stable() {
        assert_eq!(
            key_package_lookup_interest_id("peerpubkey"),
            InterestId(0xfa96_6f05_f77c_1fe2)
        );
        assert_eq!(
            group_message_interest_id("abcd", "wss://group-a/"),
            InterestId(0x65ae_a778_1d18_8e5d)
        );
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
    fn key_package_lookup_interest_targets_only_kind_30443() {
        let i = key_package_lookup_interest("peerpubkey");
        assert!(i.shape.authors.contains("peerpubkey"));
        assert!(i.shape.kinds.contains(&KIND_MARMOT_KEY_PACKAGE));
        // Legacy kind:443 is retired — must NOT appear in lookup interests.
        assert_eq!(
            i.shape.kinds.len(),
            1,
            "only kind:30443, no legacy kind:443"
        );
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
            assert!(i.shape.kinds.contains(&KIND_MARMOT_GROUP_MESSAGE));
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
