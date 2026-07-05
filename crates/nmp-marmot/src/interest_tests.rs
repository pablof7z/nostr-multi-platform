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
