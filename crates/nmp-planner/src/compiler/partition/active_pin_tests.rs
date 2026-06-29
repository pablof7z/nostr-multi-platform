//! Active-pin kind:0 resolution lane (`UserConfiguredCategory::ActivePin`).
//!
//! Proves the NIP-29 single-relay fix: a profile (kind:0-only) claim for an
//! author with no NIP-65 mailbox and no configured app/indexer relays still
//! lands on the relay the client already pins (the connected group host relay),
//! while general content interests never fan out to that relay.

use crate::{
    compiler::{EmptyMailboxCache, InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
    interest::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest},
    plan::{RoutingSource, UserConfiguredCategory},
};

const GROUP_RELAY: &str = "ws://127.0.0.1:9888";

fn pk(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

/// A profile (kind:0-only) resolution claim, exactly as
/// `kernel/requests/profile.rs::register_profile_claim_interest` builds it.
fn profile_claim(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk(author)].into_iter().collect(),
            kinds: [0u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: true,
    }
}

/// A relay-pinned NIP-29 group timeline interest (Case E). Its only routing
/// role here is to contribute `GROUP_RELAY` to the derived active-pin set.
fn pinned_group_interest(id: u64) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: [9u32].into_iter().collect(),
            relay_pin: Some(GROUP_RELAY.to_string()),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// A kind:1 timeline interest for the same author — the negative control.
fn timeline_interest(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk(author)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// The headline case: no NIP-65, no app/indexer relays, but a pinned group
/// relay is active → the kind:0 claim lands on the group relay via ActivePin,
/// and the author is NOT unroutable.
#[test]
fn kind0_claim_resolves_from_active_pinned_relay() {
    let cache = EmptyMailboxCache;
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[pinned_group_interest(1), profile_claim(2, "mallory")])
        .expect("compile");

    let group = plan
        .per_relay
        .get(GROUP_RELAY)
        .expect("kind:0 claim must land on the pinned group relay");
    assert!(
        group
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::ActivePin)),
        "the landing must be tagged ActivePin, not invented out of thin air"
    );
    // The author rides exactly one kind:0 sub-shape there.
    let serves_author = group
        .sub_shapes
        .iter()
        .any(|s| s.shape.authors.contains(&pk("mallory")) && s.shape.kinds.contains(&0));
    assert!(serves_author, "mallory's kind:0 must be requested on the relay");
    assert!(
        plan.unroutable_authors.is_empty(),
        "the pinned relay is a valid landing pad — nobody is unroutable"
    );
}

/// Scoping guard: a kind:1 timeline for the same author must NOT be fanned out
/// to the pinned group relay (no content leak to group relays).
#[test]
fn kind1_timeline_does_not_fan_out_to_pinned_relay() {
    let cache = EmptyMailboxCache;
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[pinned_group_interest(1), timeline_interest(2, "mallory")])
        .expect("compile");

    // The only entry on GROUP_RELAY is the pinned group interest itself (kind:9),
    // never the author's kind:1 — the timeline author stays unroutable here.
    if let Some(group) = plan.per_relay.get(GROUP_RELAY) {
        let leaks_kind1 = group
            .sub_shapes
            .iter()
            .any(|s| s.shape.authors.contains(&pk("mallory")) && s.shape.kinds.contains(&1));
        assert!(!leaks_kind1, "kind:1 timeline must never ride a pinned relay");
    }
    assert!(
        plan.unroutable_authors.contains(&pk("mallory")),
        "with no mailbox/app/indexer, the kind:1 author is unroutable (unchanged)"
    );
}

/// Multi-kind self-bootstrap ({0,3,10002}) is NOT the profile-claim shape, so
/// it must not ride the active-pin lane either (exact `{0}` gate).
#[test]
fn multi_kind_self_bootstrap_does_not_ride_active_pin() {
    let cache = EmptyMailboxCache;
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let interest = LogicalInterest {
        id: InterestId(2),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pk("mallory")].into_iter().collect(),
            kinds: [0u32, 3, 10002].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: true,
    };

    let plan = compiler
        .compile(&[pinned_group_interest(1), interest])
        .expect("compile");

    let on_pin = plan
        .per_relay
        .get(GROUP_RELAY)
        .map(|g| {
            g.sub_shapes
                .iter()
                .any(|s| s.shape.authors.contains(&pk("mallory")))
        })
        .unwrap_or(false);
    assert!(!on_pin, "only the exact kind:0 shape rides the active-pin lane");
}

/// Additivity: a NIP-65-known author still gets their outbox relay AND the
/// pinned group relay for the kind:0 claim (the group relay may carry a fresher
/// kind:0; outbox routing is preserved).
#[test]
fn active_pin_is_additive_to_nip65_outbox() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pk("mallory"),
        MailboxSnapshot {
            write_relays: vec!["wss://mallory-write".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[pinned_group_interest(1), profile_claim(2, "mallory")])
        .expect("compile");

    let outbox = plan
        .per_relay
        .get("wss://mallory-write")
        .expect("NIP-65 outbox relay must still be selected");
    assert!(outbox.role_tags.contains(&RoutingSource::Nip65));
    assert!(
        plan.per_relay.contains_key(GROUP_RELAY),
        "the pinned relay is added on top of the outbox relay"
    );
}

/// No pins active → behavior is exactly as before (empty active-pin set is a
/// no-op): a no-mailbox kind:0 discovery oneshot with no app/indexer relays is
/// still unroutable.
#[test]
fn no_pins_is_a_noop() {
    let cache = EmptyMailboxCache;
    let compiler = SubscriptionCompiler::new(&cache, &[]);

    let plan = compiler
        .compile(&[profile_claim(2, "mallory")])
        .expect("compile");

    assert!(plan.per_relay.is_empty(), "no relays without a pin or config");
    assert!(plan.unroutable_authors.contains(&pk("mallory")));
}
