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
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, LogicalInterest, PTagRouting};
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
///
/// `p_tag_routing` is forced to [`PTagRouting::Nip17DmRelays`] (#3057). A
/// Marmot Welcome is gift-wrapped and PUBLISHED to the invitee's verified
/// kind:10050 DM-inbox relays — the exact same `VerifiedPrivateInbox` route
/// class a NIP-17 DM uses (see `projection/ops/welcome.rs::wrap_and_publish_welcomes`
/// / `resolve_invitee_inboxes`). Without this override,
/// `ViewDependencies::into_logical_interest`'s default
/// (`PTagRouting::Nip65ReadRelays`) routed the RECEIVE side through the
/// account's public kind:10002 read relays instead — a relay set that need
/// not overlap the kind:10050 DM-inbox relays the Welcome was actually
/// delivered to. The publish and subscribe sides must agree on the SAME
/// relay-selection policy for a "verified private inbox" or the invitee's
/// client can go connected-but-deaf: it holds live sockets to other relays
/// while the delivering relay carries no matching REQ (nmp-nip17's own
/// `active_giftwrap_inbox_interest` sets this override for the identical
/// reason — see `nmp_nip17::inbox::active_giftwrap_inbox_interest`).
#[must_use]
pub fn giftwrap_inbox_interest(pubkey: &str) -> LogicalInterest {
    let deps = nmp_core::substrate::ViewDependencies {
        kinds: vec![KIND_GIFT_WRAP],
        tag_refs: vec![("p".to_string(), pubkey.to_string())],
        ..Default::default()
    };
    let mut interest = deps.into_logical_interest(
        giftwrap_interest_id(pubkey),
        nmp_planner::InterestScope::Account(pubkey.to_string()),
        InterestLifecycle::Tailing,
    );
    interest.shape.p_tag_routing = PTagRouting::Nip17DmRelays;
    interest
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

    /// #3057 regression: the Welcome gift-wrap inbox interest MUST route via
    /// kind:10050 DM-inbox relays, not the generic kind:10002 NIP-65 read
    /// relays. A Marmot Welcome is published to the invitee's verified
    /// kind:10050 DM-inbox relays (`wrap_and_publish_welcomes`); if the
    /// receive-side interest used the `PTagRouting` default
    /// (`Nip65ReadRelays`) instead, the subscription would land on a
    /// different relay set than the one the Welcome was actually delivered
    /// to, and the invitee's client would never see it — connected to other
    /// relays, but deaf on the one that matters. See
    /// `giftwrap_inbox_interest_compiles_onto_dm_relay_not_nip65_relay` below
    /// for the full planner-compile proof.
    #[test]
    fn giftwrap_inbox_interest_uses_nip17_dm_relay_routing() {
        let i = giftwrap_inbox_interest("selfpubkey");
        assert_eq!(
            i.shape.p_tag_routing,
            PTagRouting::Nip17DmRelays,
            "Marmot Welcome gift-wraps are delivered like NIP-17 DMs: the \
             receive-side interest must route through kind:10050 DM-inbox \
             relays, matching the publish-side's verified-private-inbox \
             relay selection — NOT the generic kind:10002 NIP-65 read relays"
        );
    }

    /// #3057 — end-to-end planner-compile proof of the routing bug/fix.
    ///
    /// Reproduces the production shape exactly: invitee `bob` has a kind:10002
    /// NIP-65 read-relay list that does NOT include `nos.lol`, but DOES have
    /// `nos.lol` in his kind:10050 DM-inbox list (the relay
    /// `resolve_invitee_inboxes` / `wrap_and_publish_welcomes` actually
    /// publishes Welcomes to). Compiling `giftwrap_inbox_interest("bob")`
    /// through the real `SubscriptionCompiler` must produce a subscription on
    /// `nos.lol` (where the Welcome lands) and must NOT depend on the NIP-65
    /// read relay. Before the #3057 fix (no `p_tag_routing` override) this
    /// compiled onto `wss://bob-nip65-read.example` instead — a relay that
    /// never sees the Welcome — reproducing the observed bug: the client
    /// stays connected (to its NIP-65 read relays) while never opening the
    /// REQ that would actually surface the pending Welcome.
    #[test]
    fn giftwrap_inbox_interest_compiles_onto_dm_relay_not_nip65_relay() {
        use nmp_planner::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler};

        let mut cache = InMemoryMailboxCache::new();
        cache.put(
            "bob".to_string(),
            MailboxSnapshot {
                read_relays: vec!["wss://bob-nip65-read.example".to_string()],
                ..Default::default()
            },
        );
        cache.put_dm_relays("bob".to_string(), vec!["wss://nos.lol".to_string()]);

        let compiler = SubscriptionCompiler::new(&cache, &[]);
        let interest = giftwrap_inbox_interest("bob");
        let plan = compiler
            .compile(&[interest])
            .expect("compiling a single #p gift-wrap interest must not fail");

        assert!(
            plan.per_relay.contains_key("wss://nos.lol"),
            "the gift-wrap inbox subscription must land on bob's kind:10050 \
             DM-inbox relay (nos.lol) — the same relay the Welcome is \
             published to; got relays: {:?}",
            plan.per_relay.keys().collect::<Vec<_>>()
        );
        assert!(
            !plan.per_relay.contains_key("wss://bob-nip65-read.example"),
            "the gift-wrap inbox subscription must NOT route through the \
             generic kind:10002 NIP-65 read relay — that relay never \
             receives the Welcome; got relays: {:?}",
            plan.per_relay.keys().collect::<Vec<_>>()
        );
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
