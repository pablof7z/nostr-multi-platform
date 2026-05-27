//! W7 hint-consumption tests for `case_b_addresses`.
//!
//! Tests for the Case B (addressable interests, no explicit authors) hint-
//! consumption path: `docs/design/relay-search-radius-impl-plan.md` §W7.
//!
//! Doctrine guards verified:
//!   D3 — hints become `RoutingSource::Hint`; four-lane discipline preserved.
//!   D6 — malformed hint URLs are dropped silently; no panic.
//!   D8 — hint walk is O(hints.len()); ≤1 hint per W5 oneshot in practice.

use crate::{
    compiler::{InMemoryMailboxCache, SubscriptionCompiler},
    interest::{
        HintSource, InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
        NaddrCoord, RelayHint,
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
    NaddrCoord {
        pubkey: pk(pubkey),
        kind,
        d_tag: d.to_string(),
    }
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

/// W7-5(B): A malformed (non-wss://) hint URL is silently dropped (D6).
///
/// Neither a panic nor a relay entry must result from a garbage URL.
/// The interest still compiles — only the bad hint is discarded.
#[test]
fn malformed_hint_url_silently_dropped_in_case_b() {
    let cache = InMemoryMailboxCache::new();
    let compiler = SubscriptionCompiler::with_relays(&cache, &[], &[], &[]);

    let interest = addr_interest_with_hints(
        1,
        vec![coord("gigi", 30023, "article-1")],
        vec![hint("http://not-a-relay.example")],
    );

    // Must not panic.
    let plan = compiler
        .compile(&[interest])
        .expect("compile must not fail");

    // The malformed hint must NOT produce a relay entry.
    assert!(
        plan.per_relay.get("http://not-a-relay.example").is_none(),
        "malformed hint must not produce a relay entry",
    );
    // Gigi has no valid route → still unroutable.
    assert!(
        plan.unroutable_authors.contains(&pk("gigi")),
        "gigi must remain unroutable when only hint is malformed; got {:?}",
        plan.unroutable_authors,
    );
}
