//! KeyPackage cache: `cache_key_package` / `cached_key_packages`.

use super::fixtures::{new_actor, test_relays};

/// The KeyPackage cache (populated by Marmot's ingest parser) must round
/// trip: cache a peer's signed event, then retrieve it by pubkey and list it.
#[test]
fn key_package_cache_round_trips() {
    let alice = new_actor();
    let bob = new_actor();
    let carol = new_actor();

    let bob_kp = bob.service.publish_key_package(test_relays()).unwrap();
    // Alice caches Bob's signed kind:30443 event.
    alice.service.cache_key_package(bob_kp.event_30443.clone());

    // Retrieval by pubkey returns exactly Bob's event.
    let cached = alice.service.cached_key_packages(&[bob.pubkey()]);
    assert_eq!(cached.len(), 1, "bob's kp is cached");
    assert_eq!(cached[0].pubkey, bob.pubkey());

    // Carol was never cached — she is filtered out, not returned empty-shaped.
    let mixed = alice
        .service
        .cached_key_packages(&[bob.pubkey(), carol.pubkey()]);
    assert_eq!(mixed.len(), 1, "only cached pubkeys returned");

    // cached_kp_pubkeys lists Bob's hex pubkey.
    let listed = alice.service.cached_kp_pubkeys();
    assert!(listed.contains(&bob.pubkey().to_hex()));
    assert!(!listed.contains(&carol.pubkey().to_hex()));

    // Re-caching the same author overwrites silently (newest wins; kind:30443
    // only — event_443 removed 2026-05-31).
    alice.service.cache_key_package(bob_kp.event_30443.clone());
    assert_eq!(
        alice.service.cached_key_packages(&[bob.pubkey()]).len(),
        1,
        "re-cache overwrites, never duplicates"
    );
}
