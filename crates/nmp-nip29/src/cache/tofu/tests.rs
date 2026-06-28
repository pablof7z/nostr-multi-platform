//! Tests for `TofuSignerCache` — both the in-memory contract (§4.3 step ladder)
//! and the durable-store persistence contract (restart-survival, #2286).

use nmp_store::{failing_put_domain_handle_for_test, MemEventStore};

use crate::cache::tofu::{TofuSignerCache, TrustCheckOutcome};
use crate::group_id::GroupId;

fn group() -> GroupId {
    GroupId::new("wss://h.example.com", "g1")
}

// ── In-memory contract tests ──────────────────────────────────────────────────

#[test]
fn tofu_first_39000_pins_signer() {
    let mut t = TofuSignerCache::new();
    let g = group();
    let r = t
        .evaluate(
            crate::kinds::KIND_GROUP_METADATA,
            &g,
            "relay-pk",
            "evt-id",
            1,
        )
        .unwrap();
    assert_eq!(r, TrustCheckOutcome::Accepted);
    assert_eq!(t.pinned_signer(&g), Some("relay-pk"));
    let r = t
        .evaluate(crate::kinds::KIND_GROUP_ADMINS, &g, "relay-pk", "evt-2", 2)
        .unwrap();
    assert_eq!(r, TrustCheckOutcome::Accepted);
}

#[test]
fn tofu_quarantines_39001_before_39000() {
    let mut t = TofuSignerCache::new();
    let g = group();
    let r = t
        .evaluate(crate::kinds::KIND_GROUP_ADMINS, &g, "spoofer", "evt-a", 1)
        .unwrap();
    assert_eq!(r, TrustCheckOutcome::Quarantined);
    assert_eq!(t.pinned_signer(&g), None);
    let r = t
        .evaluate(
            crate::kinds::KIND_GROUP_METADATA,
            &g,
            "relay-pk",
            "evt-b",
            2,
        )
        .unwrap();
    assert_eq!(r, TrustCheckOutcome::Accepted);
    let replayed = t.replay_quarantine(&g);
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].1, TrustCheckOutcome::Rejected);
}

#[test]
fn nip11_strict_match_rejects_mismatch() {
    let mut t = TofuSignerCache::new();
    let g = group();
    t.set_nip11_pubkey(g.host_relay_url.clone(), "declared-pk")
        .unwrap();
    let r = t
        .evaluate(crate::kinds::KIND_GROUP_METADATA, &g, "other-pk", "evt", 1)
        .unwrap();
    assert_eq!(r, TrustCheckOutcome::Rejected);
    let r = t
        .evaluate(
            crate::kinds::KIND_GROUP_METADATA,
            &g,
            "declared-pk",
            "evt",
            1,
        )
        .unwrap();
    assert_eq!(r, TrustCheckOutcome::Accepted);
}

#[test]
fn tofu_pin_write_failure_does_not_accept_or_mutate() {
    let mut cache = TofuSignerCache {
        domain: Some(failing_put_domain_handle_for_test("test.nip29.tofu")),
        ..Default::default()
    };
    let g = group();

    let err = cache
        .evaluate(
            crate::kinds::KIND_GROUP_METADATA,
            &g,
            "relay-pk",
            "evt-id",
            1,
        )
        .expect_err("store-backed first pin must fail when persistence fails");

    assert!(
        err.to_string().contains("test domain put failure"),
        "expected failing domain error, got {err:?}"
    );
    assert_eq!(cache.pinned_signer(&g), None);
}

#[test]
fn tofu_nip11_write_failure_does_not_mutate() {
    let mut cache = TofuSignerCache {
        domain: Some(failing_put_domain_handle_for_test("test.nip29.tofu")),
        ..Default::default()
    };
    let g = group();

    cache
        .set_nip11_pubkey(g.host_relay_url.clone(), "declared-pk")
        .expect_err("store-backed NIP-11 declaration must fail when persistence fails");

    assert!(!cache.nip11_pubkey.contains_key(&g.host_relay_url));
    let r = cache
        .evaluate(crate::kinds::KIND_GROUP_ADMINS, &g, "declared-pk", "evt", 1)
        .unwrap();
    assert_eq!(r, TrustCheckOutcome::Quarantined);
}

// ── Persistence tests (#2286) ─────────────────────────────────────────────────

/// Pinned signer and NIP-11 keys survive a simulated restart (re-open the
/// same MemEventStore, reconstruct the cache, verify state is intact).
#[test]
fn tofu_pinned_signer_survives_restart() {
    let store = MemEventStore::new();
    let g = group();

    // Session 1: pin a signer via the first 39000.
    {
        let mut cache = TofuSignerCache::open(&store).expect("open session 1");
        let r = cache
            .evaluate(
                crate::kinds::KIND_GROUP_METADATA,
                &g,
                "relay-pk-persist",
                "evt-1",
                100,
            )
            .unwrap();
        assert_eq!(r, TrustCheckOutcome::Accepted);
        assert_eq!(cache.pinned_signer(&g), Some("relay-pk-persist"));
    }

    // Session 2: re-open the same store — pinned signer must be loaded.
    {
        let mut cache = TofuSignerCache::open(&store).expect("open session 2");
        assert_eq!(
            cache.pinned_signer(&g),
            Some("relay-pk-persist"),
            "pinned signer must survive restart"
        );
        // A new 39000 from a DIFFERENT signer must be rejected (TOFU steady
        // state), proving we can distinguish first-pin from credential-change.
        let r = cache
            .evaluate(
                crate::kinds::KIND_GROUP_METADATA,
                &g,
                "impostor-pk",
                "evt-bad",
                200,
            )
            .unwrap();
        assert_eq!(
            r,
            TrustCheckOutcome::Rejected,
            "post-restart signer mismatch must be Rejected, not treated as first-pin"
        );
        // Legitimate signer is still accepted.
        let r = cache
            .evaluate(
                crate::kinds::KIND_GROUP_ADMINS,
                &g,
                "relay-pk-persist",
                "evt-2",
                200,
            )
            .unwrap();
        assert_eq!(r, TrustCheckOutcome::Accepted);
    }
}

/// NIP-11 declared pubkey survives restart.
#[test]
fn tofu_nip11_pubkey_survives_restart() {
    let store = MemEventStore::new();
    let g = group();

    {
        let mut cache = TofuSignerCache::open(&store).expect("open session 1");
        cache
            .set_nip11_pubkey(g.host_relay_url.clone(), "nip11-pk")
            .unwrap();
        let r = cache
            .evaluate(crate::kinds::KIND_GROUP_METADATA, &g, "nip11-pk", "e1", 1)
            .unwrap();
        assert_eq!(r, TrustCheckOutcome::Accepted);
    }

    {
        let mut cache = TofuSignerCache::open(&store).expect("open session 2");
        // NIP-11 key loaded → non-declared signer must be Rejected.
        let r = cache
            .evaluate(crate::kinds::KIND_GROUP_METADATA, &g, "wrong-pk", "e2", 2)
            .unwrap();
        assert_eq!(
            r,
            TrustCheckOutcome::Rejected,
            "NIP-11 key must survive restart"
        );
        let r = cache
            .evaluate(crate::kinds::KIND_GROUP_METADATA, &g, "nip11-pk", "e3", 3)
            .unwrap();
        assert_eq!(r, TrustCheckOutcome::Accepted);
    }
}

/// Multiple groups on different hosts all persist independently.
#[test]
fn tofu_multiple_groups_persist_independently() {
    let store = MemEventStore::new();
    let g1 = GroupId::new("wss://relay-a.example.com", "room1");
    let g2 = GroupId::new("wss://relay-b.example.com", "room2");

    {
        let mut cache = TofuSignerCache::open(&store).expect("open");
        cache
            .evaluate(crate::kinds::KIND_GROUP_METADATA, &g1, "pk-a", "e1", 1)
            .unwrap();
        cache
            .evaluate(crate::kinds::KIND_GROUP_METADATA, &g2, "pk-b", "e2", 2)
            .unwrap();
    }

    {
        let cache = TofuSignerCache::open(&store).expect("reopen");
        assert_eq!(cache.pinned_signer(&g1), Some("pk-a"));
        assert_eq!(cache.pinned_signer(&g2), Some("pk-b"));
    }
}
