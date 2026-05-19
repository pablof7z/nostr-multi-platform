//! Integration test: NIP-29 (a protocol crate) consumes the generic
//! `nmp-core` planner API and the relay-pin lane routes correctly.
//!
//! This test is intentionally a *consumer* test — it does NOT name any
//! `case_e_relay_pinned`-internal symbols. The contract under test is the
//! observable plan output:
//!
//! - A `LogicalInterest` whose `InterestShape::relay_pin = Some(host)`
//!   produces a `RelayPlan` keyed on that host.
//! - The `role_tags` for that plan include `RoutingSource::UserConfigured(Debug)`
//!   (the marker the relay-pinned partition case emits).
//! - The four-lane routing is suppressed: indexer / NIP-65 outbox / inbox
//!   relays are NOT contacted even when authors / #p tags are present on the
//!   same interest.
//! - The `nmp-nip29` protocol crate hits the same code path when its
//!   `host_pinned_interest` helper builds a `LogicalInterest` — proving the
//!   crate is purely a consumer of the generic kernel API.
//!
//! No NIP-29-specific compiler logic exists in `nmp-core`; this test would
//! pass for any future protocol crate that opts into the relay-pin lane.

use std::collections::{BTreeMap, BTreeSet};

use nmp_core::planner::{
    EmptyMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, RoutingSource, SubscriptionCompiler, UserConfiguredCategory,
};

use nmp_nip29::group_id::GroupId;
use nmp_nip29::interest::host_pinned_interest;

const HOST_A: &str = "wss://host-a.example.com";
const HOST_B: &str = "wss://host-b.example.com";
const INDEXER: &str = "wss://indexer.example.com";

fn indexer_set() -> Vec<String> {
    vec![INDEXER.to_string()]
}

/// Build a relay-pinned `LogicalInterest` using only `nmp-core` generic API —
/// no protocol-crate types. Demonstrates the boundary: any consumer can opt
/// into the third routing lane by setting `relay_pin`.
fn generic_pinned_interest(id: u64, host: &str, kinds: &[u32]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            kinds: kinds.iter().copied().collect(),
            relay_pin: Some(host.to_string()),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    }
}

#[test]
fn generic_relay_pinned_interest_routes_to_host_only() {
    // Pure generic kernel API — no nip29 imports needed for this case.
    let interest = generic_pinned_interest(1, HOST_A, &[9]);

    let cache = EmptyMailboxCache;
    let indexer = indexer_set();
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let plan = compiler.compile(&[interest]).expect("compile");

    assert!(
        plan.per_relay.contains_key(HOST_A),
        "relay-pinned interest must produce a RelayPlan for the pinned host"
    );
    assert!(
        !plan.per_relay.contains_key(INDEXER),
        "relay-pinned interest must NOT fall through to the indexer set"
    );

    let host_plan = plan.per_relay.get(HOST_A).expect("host plan present");
    assert!(
        host_plan
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Debug)),
        "Case E (relay-pinned routing) must mark the routing source as UserConfigured(Debug)"
    );

    assert_eq!(
        host_plan.sub_shapes.len(),
        1,
        "single interest → single sub_shape on the pinned host"
    );
    assert_eq!(
        host_plan.sub_shapes[0].shape.relay_pin.as_deref(),
        Some(HOST_A),
        "the per-relay sub-shape preserves the relay_pin field"
    );
}

#[test]
fn nip29_protocol_crate_consumes_generic_relay_pin_api() {
    // Same compiler behavior, but the interest is built by nip29's helper.
    // This proves the protocol crate is purely a consumer — nmp-core has no
    // group-aware code path.
    let group = GroupId::new(HOST_A, "room-x");
    let interest =
        host_pinned_interest(1, &group, [9], BTreeMap::new(), InterestLifecycle::Tailing);

    // Sanity: nip29's helper sets the generic `relay_pin` field, not any
    // protocol-specific field.
    assert_eq!(
        interest.shape.relay_pin.as_deref(),
        Some(HOST_A),
        "nip29 helper must populate the generic relay_pin field"
    );

    let cache = EmptyMailboxCache;
    let indexer = indexer_set();
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let plan = compiler.compile(&[interest]).expect("compile");

    let host_plan = plan
        .per_relay
        .get(HOST_A)
        .expect("nip29-built interest routes to the host relay");
    assert!(
        host_plan
            .role_tags
            .contains(&RoutingSource::UserConfigured(UserConfiguredCategory::Debug)),
        "nip29 path hits the same Case E as a hand-built generic interest"
    );
}

#[test]
fn different_relay_pins_emit_distinct_per_relay_plans() {
    // Two interests pinned to two different hosts: the planner must produce
    // two distinct RelayPlan entries; Rule 9 prevents any merge.
    let i_a = generic_pinned_interest(1, HOST_A, &[9]);
    let i_b = generic_pinned_interest(2, HOST_B, &[9]);

    let cache = EmptyMailboxCache;
    let indexer = indexer_set();
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let plan = compiler.compile(&[i_a, i_b]).expect("compile");

    assert!(plan.per_relay.contains_key(HOST_A), "host A must be in plan");
    assert!(plan.per_relay.contains_key(HOST_B), "host B must be in plan");
    assert!(
        !plan.per_relay.contains_key(INDEXER),
        "indexer must NOT be reached"
    );
    assert_eq!(
        plan.per_relay.len(),
        2,
        "exactly two per-relay plans, one per pinned host"
    );
}

#[test]
fn same_host_pinned_interests_coalesce_h_tag_values() {
    // Two interests pinned to the SAME host but with different `h` tag values
    // must coalesce into one per-host REQ whose `h` set is the union. This is
    // the "h-tag coalesce" pattern — Rule 9 passes (same pin), Rule 2 unions
    // the `h` values.
    fn h_tagged(id: u64, host: &str, h_val: &str) -> LogicalInterest {
        let mut tags = BTreeMap::new();
        let mut h_values: BTreeSet<String> = BTreeSet::new();
        h_values.insert(h_val.to_string());
        tags.insert("h".to_string(), h_values);
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::ActiveAccount,
            shape: InterestShape {
                kinds: [9u32].into_iter().collect(),
                tags,
                relay_pin: Some(host.to_string()),
                ..Default::default()
            },
            hints: Vec::new(),
            lifecycle: InterestLifecycle::Tailing,
        }
    }

    let i_room_a = h_tagged(1, HOST_A, "room-a");
    let i_room_b = h_tagged(2, HOST_A, "room-b");

    let cache = EmptyMailboxCache;
    let indexer = indexer_set();
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let plan = compiler.compile(&[i_room_a, i_room_b]).expect("compile");

    let host_plan = plan.per_relay.get(HOST_A).expect("host plan present");
    assert_eq!(
        host_plan.sub_shapes.len(),
        1,
        "two same-host pinned interests collapse into one per-host REQ"
    );
    let merged_h = host_plan.sub_shapes[0]
        .shape
        .tags
        .get("h")
        .expect("h tag dimension present");
    assert_eq!(
        merged_h.len(),
        2,
        "the merged sub-shape carries the union of both h values"
    );
    assert!(merged_h.contains("room-a"));
    assert!(merged_h.contains("room-b"));
}

#[test]
fn pinned_interest_with_authors_skips_outbox_lookup() {
    // Even when authors are present on the interest, the relay-pin overrides
    // the four-lane dispatch: no NIP-65 mailbox lookup, no indexer fallback.
    // This is the structural guarantee Case E provides.
    let mut interest = generic_pinned_interest(1, HOST_A, &[9]);
    interest
        .shape
        .authors
        .insert("a".repeat(64));

    let cache = EmptyMailboxCache;
    let indexer = indexer_set();
    let compiler = SubscriptionCompiler::new(&cache, &indexer);

    let plan = compiler.compile(&[interest]).expect("compile");

    // Only the pinned host appears — the author's "would-be" outbox lookup is
    // suppressed. EmptyMailboxCache has no entries, so a Case A path would
    // fall through to indexer; the relay-pin lane prevents that.
    assert!(plan.per_relay.contains_key(HOST_A));
    assert!(!plan.per_relay.contains_key(INDEXER));
    assert_eq!(plan.per_relay.len(), 1);

    // The author MUST still appear on the wire filter (relays expect it).
    let host_plan = plan.per_relay.get(HOST_A).unwrap();
    assert_eq!(host_plan.sub_shapes[0].shape.authors.len(), 1);
}
