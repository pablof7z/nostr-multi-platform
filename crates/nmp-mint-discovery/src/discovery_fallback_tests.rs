//! Fallback-root ("cold-start bootstrap trust seed") aggregation tests
//! (#3042 composition). Split out of `discovery_tests.rs` to keep each file
//! under the 500-LOC hard cap; a child of that module so it reuses its
//! `pk`/`announcement`/`recommendation`/`wot_with`/`NUTZAP` helpers via
//! `use super::*`.

use super::*;

/// A cold viewer (no ingested follows) with a configured `fallback_root`
/// routes scoring through the seed's graph instead of scoring everything at
/// 0, and every mint that counted a rerouted recommendation is labeled
/// `via_fallback`.
#[test]
fn aggregate_with_fallback_root_routes_cold_viewers_to_the_seed_and_sets_via_fallback() {
    let viewer = pk("aa"); // cold: no follows of its own
    let seed = pk("55"); // the app's curated fallback root
    let recommender = pk("bb");
    let wot = wot_with(&seed, &[&recommender], &[], &[]);

    let announcements = vec![announcement(&recommender, "https://seeded.mint", NUTZAP)];
    let recs = vec![recommendation("r1", &recommender, "https://seeded.mint")];

    let policy = DiscoveryPolicy {
        fallback_root: Some(seed.clone()),
        ..DiscoveryPolicy::default()
    };
    let mints = aggregate(&viewer, &announcements, &recs, &wot, &policy);

    assert_eq!(mints.len(), 1);
    assert_eq!(
        mints[0].trust_score, 100,
        "the seed's direct follow is scored, not the cold viewer's empty graph"
    );
    assert!(
        mints[0].via_fallback,
        "trust for a cold viewer was computed via the fallback root"
    );
}

/// Without a configured `fallback_root`, a cold viewer sees no trusted
/// mints at all — reproducing the pre-fallback behavior byte-for-byte
/// (`DiscoveryPolicy::default().fallback_root == None`).
#[test]
fn aggregate_without_fallback_root_leaves_a_cold_viewer_with_no_trust() {
    let viewer = pk("aa");
    let recommender = pk("bb");
    let wot = wot_with(&pk("55"), &[&recommender], &[], &[]);

    let announcements = vec![announcement(&recommender, "https://seeded.mint", NUTZAP)];
    let recs = vec![recommendation("r1", &recommender, "https://seeded.mint")];

    let mints = aggregate(
        &viewer,
        &announcements,
        &recs,
        &wot,
        &DiscoveryPolicy::default(),
    );
    assert_eq!(mints[0].trust_score, 0);
    assert_eq!(mints[0].recommendation_count, 0);
    assert!(!mints[0].via_fallback);
}

/// Composition-level lock for the #3042 fallback-root self-mute guard: a COLD
/// viewer (no kind:3 follows of its own) with a `fallback_root` set, where the
/// ONLY recommender of a capability-valid mint is a pubkey the VIEWER has muted
/// (kind:10000) — even though scoring reroutes to the seed and the SEED
/// follows that recommender. The viewer's own mute must still win: the
/// recommendation is dropped (trust_score 0, no distinct recommender counted),
/// so the mint never surfaces as trusted. This crate now OWNS the composition
/// of nip87 × wot-with-fallback, so the guarantee is pinned end-to-end here,
/// not just delegated to `nmp-wot`'s own `score_rooted` unit tests.
#[test]
fn a_recommender_muted_by_a_cold_viewer_never_counts_even_via_the_fallback_seed() {
    let viewer = pk("aa"); // cold: no follows of its own
    let seed = pk("55"); // the app's curated fallback root
    let recommender = pk("bb");
    // Viewer has a mute list muting the recommender, but NO follow list (cold).
    // The seed DOES follow the recommender, so a naive reroute to the seed
    // would otherwise "trust" that recommender.
    let wot = wot_with(&viewer, &[], &[(&seed, &recommender)], &[&recommender]);

    let announcements = vec![announcement(&recommender, "https://m.mint", NUTZAP)];
    let recs = vec![recommendation("r1", &recommender, "https://m.mint")];

    let policy = DiscoveryPolicy {
        fallback_root: Some(seed.clone()),
        ..DiscoveryPolicy::default()
    };
    let mints = aggregate(&viewer, &announcements, &recs, &wot, &policy);

    // The mint is still discoverable (announced + capability-valid), but the
    // muted recommender's vouch carries no weight despite the seed reroute.
    assert_eq!(mints.len(), 1);
    assert_eq!(mints[0].url, "https://m.mint");
    assert_eq!(
        mints[0].trust_score, 0,
        "the viewer's own mute must veto the recommendation even when scoring \
         rerouted to a fallback seed that follows the muted recommender"
    );
    assert_eq!(mints[0].recommendation_count, 0);
    assert!(!mints[0].via_fallback);
}
