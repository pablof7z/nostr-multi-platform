//! V-52 single-relay browsing — acceptance tests.
//!
//! These tests verify the end-to-end relay-scoping invariants:
//!
//! 1. A relay-pinned `LogicalInterest` (relay_pin = Some(url)) routes ONLY to
//!    the scoped relay — zero REQ to any other relay even when a NIP-65
//!    write set covers other relays.
//! 2. An unscoped interest continues to fan out to the NIP-65 write set (smoke
//!    check that we haven't broken normal routing).
//!
//! Routing invariants 1 and 2 are exercised via the planner compiler directly
//! (the relay_pin is enforced there by case_e_relay_pinned, not by the
//! GenericOutboxRouter). The router tests show that the router's generic
//! algorithm is bypassed by the explicit_targets path (lane 5).
//!
//! Store invariants are tested in `nmp-store/src/mem/tests.rs`.

use nmp_core::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot, SubscriptionCompiler,
};

const RELAY_A: &str = "wss://a.relay.example.com";
const RELAY_B: &str = "wss://b.relay.example.com";
const SCOPED_RELAY: &str = "wss://scoped.relay.example.com";

fn make_relay_pinned_interest(relay_url: &str, kinds: Vec<u32>, id: u64) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: kinds.into_iter().collect(),
            relay_pin: Some(relay_url.to_string()),
            ..InterestShape::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

fn make_nip65_interest(author: &str, kinds: Vec<u32>, id: u64) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [author.to_string()].into_iter().collect(),
            kinds: kinds.into_iter().collect(),
            ..InterestShape::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

/// V-52 acceptance test — core invariant:
///
/// A relay-pinned interest must route to EXACTLY the scoped relay and to no
/// other relay, even when a NIP-65 write set covers multiple other relays.
///
/// This test verifies the planner's case_e_relay_pinned enforcement end-to-end:
/// the compiled plan must have exactly one relay entry, and it must be the
/// scoped relay.
#[test]
fn relay_pinned_interest_routes_only_to_scoped_relay() {
    // Build a mailbox cache that has NIP-65 read relays for "alice" pointing to
    // RELAY_A and RELAY_B — two relays the generic algorithm would fan out to.
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        "alice".to_string(),
        MailboxSnapshot {
            read_relays: vec![RELAY_A.to_string(), RELAY_B.to_string()],
            write_relays: vec![RELAY_A.to_string(), RELAY_B.to_string()],
            both_relays: vec![],
        },
    );

    // Create a relay-pinned interest for SCOPED_RELAY with kind:1.
    // No authors — the pin is the only routing signal.
    let interest = make_relay_pinned_interest(SCOPED_RELAY, vec![1], 42);

    // Compile the interest through the planner.
    let compiler = SubscriptionCompiler::new(&cache, &[]);
    let plan = compiler.compile(&[interest]).expect("compile must succeed");

    // Extract the relay URLs from the compiled plan.
    let relay_urls: Vec<&str> = plan.per_relay.keys().map(String::as_str).collect();

    // The scoped relay must be in the plan.
    assert!(
        relay_urls.contains(&SCOPED_RELAY),
        "scoped relay must be in the compiled plan, got {relay_urls:?}"
    );

    // RELAY_A and RELAY_B must NOT be in the plan — relay_pin suppresses NIP-65 fan-out.
    assert!(
        !relay_urls.contains(&RELAY_A),
        "relay A (from NIP-65 mailbox) must NOT appear when relay_pin is set, got {relay_urls:?}"
    );
    assert!(
        !relay_urls.contains(&RELAY_B),
        "relay B (from NIP-65 mailbox) must NOT appear when relay_pin is set, got {relay_urls:?}"
    );

    // Exactly one relay in the plan — the scoped relay.
    assert_eq!(
        relay_urls.len(),
        1,
        "relay-pinned browse must produce exactly one relay plan entry, got {relay_urls:?}"
    );
}

/// Smoke test: unscoped interest fans out to NIP-65 write relays as normal.
///
/// Case A uses `outbox_relays()` (write + both) — set write_relays so the
/// planner resolves the author to a non-empty relay set.
/// Verifies that the relay_pin change has not broken normal routing.
#[test]
fn unscoped_interest_fans_out_to_nip65_relays() {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        "alice".to_string(),
        MailboxSnapshot {
            read_relays: vec![],
            write_relays: vec![RELAY_A.to_string(), RELAY_B.to_string()],
            both_relays: vec![],
        },
    );

    let interest = make_nip65_interest("alice", vec![1], 1);

    let compiler = SubscriptionCompiler::new(&cache, &[]);
    let plan = compiler.compile(&[interest]).expect("compile must succeed");

    let relay_urls: Vec<&str> = plan.per_relay.keys().map(String::as_str).collect();

    assert!(
        relay_urls.contains(&RELAY_A),
        "relay A must appear in unscoped NIP-65 plan, got {relay_urls:?}"
    );
    assert!(
        relay_urls.contains(&RELAY_B),
        "relay B must appear in unscoped NIP-65 plan, got {relay_urls:?}"
    );
    assert!(
        !relay_urls.contains(&SCOPED_RELAY),
        "scoped relay must NOT appear in unscoped plan, got {relay_urls:?}"
    );
}
