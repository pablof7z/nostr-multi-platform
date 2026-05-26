//! NIP-17 gift-wrap inbox routing must use kind:10050 DM relays.

use std::collections::BTreeMap;

use nmp_core::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot, PTagRouting, RoutingSource, SubscriptionCompiler,
};

fn pk(label: &str) -> String {
    format!("{label:0>64}").chars().take(64).collect()
}

fn giftwrap_dm_inbox_interest(id: u64, pubkey: &str) -> LogicalInterest {
    let mut tags = BTreeMap::new();
    tags.insert("p".to_string(), [pubkey.to_string()].into_iter().collect());
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            kinds: [1059].into_iter().collect(),
            tags,
            p_tag_routing: PTagRouting::Nip17DmRelays,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

#[test]
fn giftwrap_inbox_routes_to_kind10050_dm_relays_only() {
    let account = pk("account");
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        account.clone(),
        MailboxSnapshot {
            read_relays: vec!["wss://public-read.example".to_string()],
            ..Default::default()
        },
    );
    cache.put_dm_relays(account.clone(), vec!["wss://dm-only.example".to_string()]);

    let compiler = SubscriptionCompiler::new(&cache, &[]);
    let plan = compiler
        .compile(&[giftwrap_dm_inbox_interest(77, &account)])
        .expect("compile");

    assert!(
        plan.per_relay.contains_key("wss://dm-only.example"),
        "kind:10050 DM relay must carry the gift-wrap inbox REQ",
    );
    assert!(
        !plan.per_relay.contains_key("wss://public-read.example"),
        "NIP-65 read relay must not carry a NIP-17 gift-wrap inbox REQ",
    );
    assert!(
        plan.per_relay["wss://dm-only.example"]
            .role_tags
            .contains(&RoutingSource::Nip17DmRelay),
        "routing source must identify the kind:10050 DM lane",
    );
}

#[test]
fn giftwrap_inbox_fails_closed_without_kind10050_relays() {
    let account = pk("account");
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        account.clone(),
        MailboxSnapshot {
            read_relays: vec!["wss://public-read.example".to_string()],
            ..Default::default()
        },
    );

    let compiler = SubscriptionCompiler::new(&cache, &[]);
    let plan = compiler
        .compile(&[giftwrap_dm_inbox_interest(78, &account)])
        .expect("compile");

    assert!(
        plan.per_relay.is_empty(),
        "missing kind:10050 must fail closed instead of falling back to NIP-65 read relays",
    );
}
