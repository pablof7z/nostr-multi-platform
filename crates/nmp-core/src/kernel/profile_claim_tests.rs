//! Tests for the M2 registry-backed profile-claim path.
//!
//! `claim_profile` registers a kind:0 `LogicalInterest` through the
//! `InterestRegistry`; the planner emits the wire REQ on the next
//! `drain_lifecycle_outbound`. These tests assert the migrated behaviour:
//!
//! * a claim for an author with a cached kind:10002 routes the kind:0 to that
//!   author's own write relays (warm outbox);
//! * `Live` claims register a Tailing kind:0 sub; mixed `CacheOk` + `Live`
//!   on one pubkey resolve to Tailing (Live wins);
//! * multi-consumer refcount keeps one deduped interest live until the last
//!   consumer releases;
//! * the F-TTL `force` re-verify of a cached profile still fires.
//!
//! The discovery / kind:10002-probe / indexer-reconnect tests (cold-start
//! routing, retry-on-miss, the #1436 redundant-connect regression, batched
//! coalescing, nprofile hints) live in the sibling `profile_claim_discovery_tests`
//! module — split out for the 500 LOC file-size hard ceiling. The
//! `claimed_profiles` projection invariants live in `profile_claim_projection_tests`.

use super::profile_claim_test_support::{drain_reqs, hex64, kind0_req_relays_for};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

// ─── (a) cached kind:10002 → kind:0 routes to author's own write relays ──────

#[test]
fn cached_nip65_profile_claim_routes_kind0_to_author_write_relays() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    kernel.relay_connected(RelayRole::Indexer);

    let alice = hex64("a11ce");
    let alice_relay = "wss://alice-write.example";
    kernel.seed_mailbox_relay_list(&alice, vec![], vec![alice_relay.to_string()], vec![]);

    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "view-0".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );

    let reqs = drain_reqs(&mut kernel);
    let relays = kind0_req_relays_for(&reqs, &alice);
    assert!(
        relays.iter().any(|u| u == alice_relay),
        "warm kind:0 claim must route to the author's NIP-65 write relay {alice_relay}; got {relays:?}"
    );
}

// ─── (c-liveness) Live registers Tailing; mixed CacheOk + Live → Tailing ─────

#[test]
fn live_claim_registers_tailing_interest() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("11e");
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "profile-view".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::Live.into(),
            false,
            Vec::new(),
        );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(true),
        "a Live claim must register a Tailing kind:0 interest"
    );
}

#[test]
fn cache_ok_claim_registers_oneshot_interest() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("0a0");
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "avatar".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(false),
        "a CacheOk claim must register a OneShot kind:0 interest"
    );
}

#[test]
fn mixed_liveness_resolves_to_tailing_live_wins() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("a11");

    // A feed avatar claims CacheOk first.
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "avatar".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(false),
        "first CacheOk claim → OneShot"
    );

    // The profile screen then claims Live for the same pubkey → upgrade.
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "profile-view".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::Live.into(),
            false,
            Vec::new(),
        );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(true),
        "a Live claim must upgrade the shared slot to Tailing (Live wins)"
    );

    // A later CacheOk claim must NOT downgrade it while a Live claim is held.
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "avatar-2".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );
    assert_eq!(
        kernel.profile_claim_interest_lifecycle_for_test(&alice),
        Some(true),
        "a later CacheOk claim must not downgrade a Tailing slot"
    );
}

// ─── (b-refcount) multi-consumer dedup: one interest, refcounted ─────────────

#[test]
fn multi_consumer_claim_dedups_to_one_interest_until_last_release() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("c0c0");

    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "view-A".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "view-B".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::CacheOk.into(),
            false,
            Vec::new(),
        );

    // Two consumers → exactly one kind:0 claim interest.
    let kind0_claim_count = kernel
        .lifecycle_mut()
        .registry()
        .iter_active()
        .iter()
        .filter(|i| {
            i.shape.kinds.len() == 1
                && i.shape.kinds.contains(&0)
                && i.shape.authors.contains(&alice)
        })
        .count();
    assert_eq!(
        kind0_claim_count, 1,
        "two consumers of one pubkey must dedup to ONE kind:0 interest"
    );

    // First consumer releases — interest stays (B still holds).
    let _ = kernel.release_ref(RefNamespace::Profile, &alice, "view-A");
    assert!(
        kernel.profile_claim_interest_registered_for_test(&alice),
        "interest must survive while a second consumer holds it"
    );

    // Last consumer releases — interest is dropped.
    let _ = kernel.release_ref(RefNamespace::Profile, &alice, "view-B");
    assert!(
        !kernel.profile_claim_interest_registered_for_test(&alice),
        "interest must be dropped once the last consumer releases"
    );
}

// ─── (c) F-TTL force re-verify of a cached profile still fires ───────────────

#[test]
fn force_reverify_of_cached_profile_enqueues_reverify() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let alice = hex64("f0f0");

    // Seed a resident kind:0 so the cached branch runs.
    let event = nostr::NostrEvent {
        id: hex64("c0"),
        pubkey: alice.clone(),
        created_at: 1_700_000_000,
        kind: 0,
        tags: vec![],
        content: r#"{"display_name":"Alice"}"#.to_string(),
        sig: String::new(),
    };
    kernel.inject_profile(event);

    let before = kernel.pending_reverify_len();
    // force = true → unconditional F-TTL re-verify enqueue.
    let _ = kernel.resolve_ref(
            RefNamespace::Profile,
            alice.clone(),
            "profile-view".to_string(),
            RefShape::Profile(ProfileShape::Card),
            RefLiveness::Live.into(),
            true,
            Vec::new(),
        );
    let after = kernel.pending_reverify_len();
    assert!(
        after > before,
        "force=true on a cached profile must enqueue an F-TTL re-verify"
    );
}
