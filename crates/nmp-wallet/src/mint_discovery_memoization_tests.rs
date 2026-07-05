//! Memoization tests for [`MintDiscoveryStore`] (hot-path safety, #2880 review
//! follow-up). Split out of `mint_discovery_tests.rs` to keep each file under
//! the 500-LOC hard cap; a child of that module so it reuses its `pk`/`kev`
//! helpers and can read the store's private `cached`/`compute_count` fields.
//!
//! Proves the memoization contract `wallet_merged_typed_projection` relies on:
//! a clean `snapshot()` serves the cache (no recompute), and EVERY input
//! mutation re-dirties it while inert events do not.

use super::*;

/// Ingest a capability-qualifying, trusted-follow-recommended mint into a
/// viewer-scoped store — the common fixture the memoization tests share.
fn store_with_one_discovered_mint() -> (MintDiscoveryStore, String) {
    let viewer = pk("aa");
    let follow = pk("bb");
    let mut store = MintDiscoveryStore::new();
    store.set_viewer(Some(viewer.clone()));
    store.ingest_kernel_event(&kev(
        1,
        &viewer,
        KIND_CONTACT_LIST,
        100,
        vec![vec!["p".to_string(), follow.clone()]],
        "",
    ));
    store.ingest_kernel_event(&kev(
        2,
        &follow,
        KIND_MINT_ANNOUNCE,
        101,
        vec![
            vec!["d".to_string(), "mint-a".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
            vec!["nuts".to_string(), "1,2,4,7,11,12".to_string()],
        ],
        "",
    ));
    store.ingest_kernel_event(&kev(
        3,
        &follow,
        KIND_MINT_RECOMMEND,
        102,
        vec![
            vec!["k".to_string(), "38172".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
        ],
        "",
    ));
    (store, follow)
}

#[test]
fn snapshot_serves_the_cache_when_clean_and_recomputes_only_when_dirty() {
    let (mut store, follow) = store_with_one_discovered_mint();

    // Ingestion left the cache dirty; the first snapshot computes exactly once.
    assert!(store.cached.is_none(), "mutations leave the cache dirty");
    let first = store.snapshot();
    assert_eq!(store.compute_count, 1);
    assert!(store.cached.is_some(), "snapshot re-populates the cache");
    assert_eq!(first.mints.len(), 1);

    // Repeated clean snapshots are served from the cache — no recompute.
    let second = store.snapshot();
    let third = store.snapshot();
    assert_eq!(store.compute_count, 1, "a clean snapshot must not recompute");
    assert_eq!(first, second);
    assert_eq!(first, third);

    // A mutation (a second trusted recommender for the same mint) re-dirties.
    store.ingest_kernel_event(&kev(
        4,
        &follow, // same author, different event id -> still one distinct recommender
        KIND_MINT_RECOMMEND,
        103,
        vec![
            vec!["k".to_string(), "38172".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
        ],
        "",
    ));
    assert!(store.cached.is_none(), "a mutation re-dirties the cache");
    let after = store.snapshot();
    assert_eq!(store.compute_count, 2, "exactly one recompute after a mutation");
    assert_eq!(after.mints.len(), 1);
}

#[test]
fn a_noop_set_viewer_does_not_dirty_the_cache() {
    let (mut store, _follow) = store_with_one_discovered_mint();
    let viewer = pk("aa");

    let _ = store.snapshot();
    assert_eq!(store.compute_count, 1);
    assert!(store.cached.is_some());

    // Setting the SAME viewer is a no-op: the cache must survive.
    store.set_viewer(Some(viewer));
    assert!(
        store.cached.is_some(),
        "re-setting the identical viewer must not invalidate the memo"
    );
    let _ = store.snapshot();
    assert_eq!(store.compute_count, 1, "no recompute after a no-op set_viewer");

    // Switching to a DIFFERENT viewer dirties it.
    store.set_viewer(Some(pk("ff")));
    assert!(store.cached.is_none(), "a real viewer change dirties the cache");
}

/// Every input `aggregate_discovered_mints` reads must dirty the cache when it
/// actually changes: the scoring viewer, an announcement, a recommendation,
/// and the follow/mute WoT graph. Non-mutating events must NOT dirty it.
#[test]
fn every_mutation_path_dirties_the_cache_and_inert_events_do_not() {
    let viewer = pk("aa");
    let follow = pk("bb");

    // 1. set_viewer (None -> Some) dirties.
    let mut store = MintDiscoveryStore::new();
    let _ = store.snapshot();
    assert!(store.cached.is_some());
    store.set_viewer(Some(viewer.clone()));
    assert!(store.cached.is_none(), "set_viewer change dirties");

    // 2. kind:3 follow-list ingest dirties.
    let _ = store.snapshot();
    store.ingest_kernel_event(&kev(
        1,
        &viewer,
        KIND_CONTACT_LIST,
        100,
        vec![vec!["p".to_string(), follow.clone()]],
        "",
    ));
    assert!(store.cached.is_none(), "contact-list ingest dirties");

    // 3. kind:38172 announcement ingest dirties.
    let _ = store.snapshot();
    store.ingest_kernel_event(&kev(
        2,
        &follow,
        KIND_MINT_ANNOUNCE,
        101,
        vec![
            vec!["d".to_string(), "mint-a".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
            vec!["nuts".to_string(), "1,2,4,7,11,12".to_string()],
        ],
        "",
    ));
    assert!(store.cached.is_none(), "announcement ingest dirties");

    // 4. kind:38000 recommendation ingest dirties.
    let _ = store.snapshot();
    store.ingest_kernel_event(&kev(
        3,
        &follow,
        KIND_MINT_RECOMMEND,
        102,
        vec![
            vec!["k".to_string(), "38172".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
        ],
        "",
    ));
    assert!(store.cached.is_none(), "recommendation ingest dirties");

    // 5. kind:10000 mute-list ingest dirties.
    let _ = store.snapshot();
    store.ingest_kernel_event(&kev(
        4,
        &viewer,
        KIND_MUTE_LIST,
        103,
        vec![vec!["p".to_string(), pk("cc")]],
        "",
    ));
    assert!(store.cached.is_none(), "mute-list ingest dirties");

    // 6. An unrelated kind (kind:1) is inert: it must NOT dirty the cache.
    let _ = store.snapshot();
    assert!(store.cached.is_some());
    store.ingest_kernel_event(&kev(5, &follow, 1, 104, vec![], "gm"));
    assert!(
        store.cached.is_some(),
        "an ignored non-discovery, non-graph kind must not invalidate the memo"
    );

    // 7. A stale (older, same-coordinate) announcement that does NOT replace is
    //    inert too.
    store.ingest_kernel_event(&kev(
        6,
        &follow,
        KIND_MINT_ANNOUNCE,
        50, // older than the created_at=101 announcement already stored
        vec![
            vec!["d".to_string(), "mint-a".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
            vec!["nuts".to_string(), "1,2".to_string()],
        ],
        "",
    ));
    assert!(
        store.cached.is_some(),
        "an older same-coordinate announcement that does not replace must not dirty"
    );
}
