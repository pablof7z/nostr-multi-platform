//! W7 hint-consumption tests for `case_a_authors` and `case_b_addresses`.
//!
//! These tests drive the failing-first TDD cycle for
//! `docs/design/relay-search-radius-impl-plan.md` §W7.
//!
//! All five tests are RED until the hint-walk logic is wired in
//! `case_a_authors::route` and `case_b_addresses::route`.
//!
//! Doctrine guards verified:
//!   D3 — hints become `RoutingSource::Hint`; four-lane discipline preserved.
//!   D6 — malformed hint URLs are dropped silently; no panic.
//!   D8 — hint walk is O(hints.len()); ≤1 hint per W5 oneshot in practice.

use crate::{
    compiler::{InMemoryMailboxCache, MailboxSnapshot, SubscriptionCompiler},
    interest::{
        HintSource, InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
        NaddrCoord, RelayHint, RelayUrl,
    },
    plan::RoutingSource,
};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn pk(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

fn hint(url: &str) -> RelayHint {
    RelayHint {
        url: url.to_string(),
        source: HintSource::UserConfigured,
    }
}

fn authors_interest_with_hints(
    id: u64,
    authors: &[&str],
    hints: Vec<RelayHint>,
) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|a| pk(a)).collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints,
        lifecycle: InterestLifecycle::Tailing,
    }
}

fn addr_interest_with_hints(
    id: u64,
    coords: Vec<NaddrCoord>,
    hints: Vec<RelayHint>,
) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            addresses: coords.into_iter().collect(),
            kinds: [30023u32].into_iter().collect(),
            ..Default::default()
        },
        hints,
        lifecycle: InterestLifecycle::OneShot,
    }
}

fn coord(pubkey: &str, kind: u32, d: &str) -> NaddrCoord {
    NaddrCoord { pubkey: pk(pubkey), kind, d_tag: d.to_string() }
}

// ─── case_a tests ─────────────────────────────────────────────────────────────

/// W7-1: Author with NO NIP-65 mailbox but a configured hint → the hint relay
/// receives a `RelayEntry` with `RoutingSource::Hint`.
///
/// Demonstrates the baseline routing path: hint alone is sufficient for
/// an interest to leave `unroutable_authors`.
#[test]
fn single_user_configured_hint_routes_to_that_relay_in_case_a() {
    let cache = InMemoryMailboxCache::new();
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

    let interest = authors_interest_with_hints(
        1,
        &["alice"],
        vec![hint("wss://hint-relay.example")],
    );

    let plan = compiler.compile(&[interest]).expect("compile");

    let entry = plan
        .per_relay
        .get("wss://hint-relay.example")
        .expect("hint relay must appear in plan");
    assert!(
        entry.role_tags.contains(&RoutingSource::Hint),
        "relay entry sourced from hint must carry RoutingSource::Hint; got {:?}",
        entry.role_tags,
    );
    // Alice is NOT unroutable — the hint carried her.
    assert!(
        plan.unroutable_authors.is_empty(),
        "alice must NOT be unroutable when a hint routes her; got {:?}",
        plan.unroutable_authors,
    );
}

/// W7-2: Author with a known NIP-65 mailbox AND a hint pointing at a
/// *different* relay → both relays appear with their respective lanes.
///
/// The four-lane discipline (D3) requires that NIP-65 and Hint remain
/// separate lanes — neither collapses the other.
#[test]
fn hint_routes_independently_of_nip65_outbox() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pk("alice"),
        MailboxSnapshot {
            write_relays: vec!["wss://alice-outbox.example".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

    let interest = authors_interest_with_hints(
        1,
        &["alice"],
        vec![hint("wss://alice-hint.example")],
    );

    let plan = compiler.compile(&[interest]).expect("compile");

    // NIP-65 lane on the outbox relay.
    let outbox = plan
        .per_relay
        .get("wss://alice-outbox.example")
        .expect("NIP-65 outbox must appear");
    assert!(
        outbox.role_tags.contains(&RoutingSource::Nip65),
        "outbox relay must carry Nip65; got {:?}",
        outbox.role_tags,
    );
    assert!(
        !outbox.role_tags.contains(&RoutingSource::Hint),
        "outbox-only relay must NOT carry Hint; got {:?}",
        outbox.role_tags,
    );

    // Hint lane on the hint relay.
    let hint_relay = plan
        .per_relay
        .get("wss://alice-hint.example")
        .expect("hint relay must appear");
    assert!(
        hint_relay.role_tags.contains(&RoutingSource::Hint),
        "hint relay must carry Hint; got {:?}",
        hint_relay.role_tags,
    );
    assert!(
        !hint_relay.role_tags.contains(&RoutingSource::Nip65),
        "hint-only relay must NOT carry Nip65; got {:?}",
        hint_relay.role_tags,
    );

    // No unroutable authors.
    assert!(plan.unroutable_authors.is_empty());
}

/// W7-3: Hint URL that matches the author's existing NIP-65 outbox →
/// produces ONE `RelayEntry` with BOTH `Nip65` AND `Hint` in `role_tags`,
/// not two separate entries.
///
/// This tests the dedup invariant: the `BTreeMap` accumulator merges sources
/// rather than duplicating entries per lane.
#[test]
fn hint_dedup_against_existing_route_in_case_a() {
    let shared_url: RelayUrl = "wss://shared-relay.example".to_string();
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pk("alice"),
        MailboxSnapshot {
            write_relays: vec![shared_url.clone()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

    // Hint points at the SAME URL as the NIP-65 outbox.
    let interest = authors_interest_with_hints(
        1,
        &["alice"],
        vec![hint(&shared_url)],
    );

    let plan = compiler.compile(&[interest]).expect("compile");

    // Exactly one entry for the shared URL.
    let entry = plan
        .per_relay
        .get(&shared_url)
        .expect("shared relay must appear");
    assert!(
        entry.role_tags.contains(&RoutingSource::Nip65),
        "shared relay must carry Nip65; got {:?}",
        entry.role_tags,
    );
    assert!(
        entry.role_tags.contains(&RoutingSource::Hint),
        "shared relay must also carry Hint; got {:?}",
        entry.role_tags,
    );

    // Only one relay total — dedup prevents creating a second entry.
    assert_eq!(
        plan.per_relay.len(),
        1,
        "deduplicated hint must not create a second entry; got {:?}",
        plan.per_relay.keys().collect::<Vec<_>>(),
    );

    assert!(plan.unroutable_authors.is_empty());
}

// ─── case_b tests ─────────────────────────────────────────────────────────────

/// W7-4: Addressable interest (kind:30023) with a hint → the hint relay
/// receives a `RelayEntry` with `RoutingSource::Hint`.
///
/// Mirrors the case_a baseline for the case_b (addressable) routing path.
#[test]
fn case_b_addressable_with_hint_routes_per_hint() {
    let cache = InMemoryMailboxCache::new();
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

    let interest = addr_interest_with_hints(
        1,
        vec![coord("gigi", 30023, "article-1")],
        vec![hint("wss://gigi-hint.example")],
    );

    let plan = compiler.compile(&[interest]).expect("compile");

    let entry = plan
        .per_relay
        .get("wss://gigi-hint.example")
        .expect("hint relay must appear in case_b plan");
    assert!(
        entry.role_tags.contains(&RoutingSource::Hint),
        "case_b hint relay must carry RoutingSource::Hint; got {:?}",
        entry.role_tags,
    );

    // coord.pubkey (gigi) is NOT unroutable — the hint carried her.
    assert!(
        plan.unroutable_authors.is_empty(),
        "gigi must NOT be unroutable when a hint routes her coord; got {:?}",
        plan.unroutable_authors,
    );
}

/// W7-5: A malformed (non-wss://) hint URL is silently dropped (D6).
///
/// Neither a panic nor a relay entry must result from a garbage URL.
/// The interest still compiles — only the bad hint is discarded.
#[test]
fn malformed_hint_url_silently_dropped() {
    let cache = InMemoryMailboxCache::new();
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

    // "http://not-a-relay" is malformed for a wss relay hint.
    let interest = authors_interest_with_hints(
        1,
        &["alice"],
        vec![hint("http://not-a-relay.example")],
    );

    // Must not panic.
    let plan = compiler.compile(&[interest]).expect("compile must not fail");

    // The malformed hint must NOT produce a relay entry.
    assert!(
        plan.per_relay.get("http://not-a-relay.example").is_none(),
        "malformed hint must not produce a relay entry",
    );
    // Alice has no valid route → still unroutable (hint was the only potential route).
    assert!(
        plan.unroutable_authors.contains(&pk("alice")),
        "alice must remain unroutable when only hint is malformed; got {:?}",
        plan.unroutable_authors,
    );
}
