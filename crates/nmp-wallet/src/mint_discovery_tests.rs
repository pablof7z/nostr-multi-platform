//! Tests for the NIP-87 web-of-trust-scoped, fail-closed mint discovery
//! aggregation (issue #2880).

use super::*;
use nmp_core::substrate::KernelEvent;
use nmp_nip87::{MintAnnouncement, MintCapabilities, MintRecommendation};
use nmp_wot::WotGraph;

fn pk(byte: &str) -> String {
    byte.repeat(32)
}

fn caps(nuts: &[u16]) -> MintCapabilities {
    MintCapabilities {
        nuts: nuts.iter().copied().collect(),
        units: BTreeSet::new(),
    }
}

/// Announcement for `url` authored by `author`, advertising `nuts`.
fn announcement(author: &str, url: &str, nuts: &[u16]) -> MintAnnouncement {
    MintAnnouncement {
        event_id: format!("evt-{url}"),
        author: author.to_string(),
        d_identifier: format!("d-{url}"),
        mint_urls: vec![url.to_string()],
        relays: vec![],
        networks: vec![],
        name: Some(format!("Mint {url}")),
        description: None,
        capabilities: caps(nuts),
    }
}

fn recommendation(event_id: &str, author: &str, url: &str) -> MintRecommendation {
    MintRecommendation {
        event_id: event_id.to_string(),
        author: author.to_string(),
        mint_coordinates: vec![],
        mint_urls: vec![url.to_string()],
        content: String::new(),
    }
}

const NUTZAP: &[u16] = &[1, 2, 4, 7, 11, 12];

fn wot_with(viewer: &str, follows: &[&str], follows_of: &[(&str, &str)], mutes: &[&str]) -> WotGraph {
    let mut wot = WotGraph::default();
    let follow_tags: Vec<Vec<String>> = follows
        .iter()
        .map(|f| vec!["p".to_string(), (*f).to_string()])
        .collect();
    wot.ingest_follow_list(viewer, &follow_tags);
    for (author, followed) in follows_of {
        wot.ingest_follow_list(
            author,
            &[vec!["p".to_string(), (*followed).to_string()]],
        );
    }
    if !mutes.is_empty() {
        let mute_tags: Vec<Vec<String>> = mutes
            .iter()
            .map(|m| vec!["p".to_string(), (*m).to_string()])
            .collect();
        wot.ingest_mute_list(viewer, &mute_tags);
    }
    wot
}

#[test]
fn recommendation_from_a_stranger_is_ignored() {
    let viewer = pk("aa");
    let stranger = pk("cc");
    let wot = wot_with(&viewer, &[], &[], &[]);

    let announcements = vec![announcement(&stranger, "https://x.mint", NUTZAP)];
    let recs = vec![recommendation("r1", &stranger, "https://x.mint")];

    let mints = aggregate_discovered_mints(
        &viewer,
        &announcements,
        &recs,
        &wot,
        &MintDiscoveryPolicy::default(),
    );
    // The mint is still discoverable (announced + capable), but the stranger's
    // vouch carries no trust weight.
    assert_eq!(mints.len(), 1);
    assert_eq!(mints[0].trust_score, 0);
    assert_eq!(mints[0].recommendation_count, 0);
}

#[test]
fn wot_scoped_recommendations_rank_by_trust() {
    let viewer = pk("aa");
    let follow = pk("bb");
    let mid = pk("0b");
    let second = pk("ee");
    // viewer follows `follow` and `mid`; `mid` follows `second` -> second-degree.
    let wot = wot_with(&viewer, &[&follow, &mid], &[(&mid, &second)], &[]);

    let announcements = vec![
        announcement(&follow, "https://followed.mint", NUTZAP),
        announcement(&second, "https://second.mint", NUTZAP),
    ];
    let recs = vec![
        recommendation("r1", &follow, "https://followed.mint"),
        recommendation("r2", &second, "https://second.mint"),
    ];

    let mints = aggregate_discovered_mints(
        &viewer,
        &announcements,
        &recs,
        &wot,
        &MintDiscoveryPolicy::default(),
    );
    assert_eq!(mints.len(), 2);
    // Direct-follow vouch (score 100) outranks the second-degree vouch (10).
    assert_eq!(mints[0].url, "https://followed.mint");
    assert_eq!(mints[0].trust_score, 100);
    assert_eq!(mints[1].url, "https://second.mint");
    assert_eq!(mints[1].trust_score, 10);
}

#[test]
fn a_muted_recommender_is_dropped() {
    let viewer = pk("aa");
    let muted = pk("dd");
    let wot = wot_with(&viewer, &[], &[], &[&muted]);

    let announcements = vec![announcement(&muted, "https://m.mint", NUTZAP)];
    let recs = vec![recommendation("r1", &muted, "https://m.mint")];

    let mints = aggregate_discovered_mints(
        &viewer,
        &announcements,
        &recs,
        &wot,
        &MintDiscoveryPolicy::default(),
    );
    assert_eq!(mints[0].trust_score, 0);
    assert_eq!(mints[0].recommendation_count, 0);
}

#[test]
fn mint_missing_nutzap_nuts_is_excluded() {
    let viewer = pk("aa");
    let follow = pk("bb");
    let wot = wot_with(&viewer, &[&follow], &[], &[]);

    // Advertises NUT-11 but not NUT-12 -> not nutzap-capable -> fail closed.
    let announcements = vec![announcement(&follow, "https://weak.mint", &[1, 2, 11])];
    let recs = vec![recommendation("r1", &follow, "https://weak.mint")];

    let mints = aggregate_discovered_mints(
        &viewer,
        &announcements,
        &recs,
        &wot,
        &MintDiscoveryPolicy::default(),
    );
    assert!(
        mints.is_empty(),
        "a mint missing NUT-12 must be excluded even when a trusted follow recommends it"
    );
}

#[test]
fn recommendation_for_unannounced_mint_is_dropped() {
    let viewer = pk("aa");
    let follow = pk("bb");
    let wot = wot_with(&viewer, &[&follow], &[], &[]);

    // No announcement for this URL -> capabilities unknown -> fail closed.
    let recs = vec![recommendation("r1", &follow, "https://ghost.mint")];

    let mints = aggregate_discovered_mints(
        &viewer,
        &[],
        &recs,
        &wot,
        &MintDiscoveryPolicy::default(),
    );
    assert!(mints.is_empty());
}

#[test]
fn one_recommender_counts_once_per_mint() {
    let viewer = pk("aa");
    let follow = pk("bb");
    let wot = wot_with(&viewer, &[&follow], &[], &[]);

    let announcements = vec![announcement(&follow, "https://x.mint", NUTZAP)];
    // Same author vouches twice (two events) for the same mint.
    let recs = vec![
        recommendation("r1", &follow, "https://x.mint"),
        recommendation("r2", &follow, "https://x.mint"),
    ];

    let mints = aggregate_discovered_mints(
        &viewer,
        &announcements,
        &recs,
        &wot,
        &MintDiscoveryPolicy::default(),
    );
    assert_eq!(mints[0].recommendation_count, 1);
    assert_eq!(mints[0].trust_score, 100);
}

#[test]
fn recommendation_via_a_tag_coordinate_resolves_to_url() {
    let viewer = pk("aa");
    let follow = pk("bb");
    let wot = wot_with(&viewer, &[&follow], &[], &[]);

    let ann = announcement(&follow, "https://coord.mint", NUTZAP);
    let coordinate = ann.coordinate();
    let mut rec = recommendation("r1", &follow, "");
    rec.mint_urls.clear();
    rec.mint_coordinates = vec![coordinate];

    let mints = aggregate_discovered_mints(
        &viewer,
        &[ann],
        &[rec],
        &wot,
        &MintDiscoveryPolicy::default(),
    );
    assert_eq!(mints.len(), 1);
    assert_eq!(mints[0].trust_score, 100);
}

// ---- MintDiscoveryStore (event ingestion) -------------------------------

fn kev(id_byte: u8, author: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>, content: &str) -> KernelEvent {
    KernelEvent {
        id: format!("{id_byte:064x}"),
        author: author.to_string(),
        kind,
        created_at,
        tags,
        content: content.to_string(),
        relay_provenance: vec![],
    }
}

#[test]
fn store_ingests_events_and_produces_scoped_projection() {
    let viewer = pk("aa");
    let follow = pk("bb");
    let stranger = pk("cc");

    let mut store = MintDiscoveryStore::new();
    store.set_viewer(Some(viewer.clone()));

    // Viewer's follow list (kind:3) -> builds the WoT graph inside the store.
    store.ingest_kernel_event(&kev(
        1,
        &viewer,
        KIND_CONTACT_LIST,
        100,
        vec![vec!["p".to_string(), follow.clone()]],
        "",
    ));

    // Two announcements (kind:38172), both nutzap-capable.
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
        &stranger,
        KIND_MINT_ANNOUNCE,
        102,
        vec![
            vec!["d".to_string(), "mint-b".to_string()],
            vec!["u".to_string(), "https://b.mint".to_string()],
            vec!["nuts".to_string(), "1,2,11,12".to_string()],
        ],
        "",
    ));

    // Recommendation from the followed account for mint A; from a stranger for B.
    store.ingest_kernel_event(&kev(
        4,
        &follow,
        KIND_MINT_RECOMMEND,
        103,
        vec![
            vec!["k".to_string(), "38172".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
        ],
        "",
    ));
    store.ingest_kernel_event(&kev(
        5,
        &stranger,
        KIND_MINT_RECOMMEND,
        104,
        vec![
            vec!["k".to_string(), "38172".to_string()],
            vec!["u".to_string(), "https://b.mint".to_string()],
        ],
        "",
    ));

    let projection = store.snapshot();
    // Both mints discoverable & capable; A carries the trusted vouch, so ranks first.
    assert_eq!(projection.mints.len(), 2);
    assert_eq!(projection.mints[0].url, "https://a.mint");
    assert_eq!(projection.mints[0].trust_score, 100);
    assert_eq!(projection.mints[1].url, "https://b.mint");
    assert_eq!(projection.mints[1].trust_score, 0);
    assert!(projection.mints.iter().all(|m| m.supports_nutzap));
}

#[test]
fn store_projection_empty_without_viewer() {
    let mut store = MintDiscoveryStore::new();
    store.ingest_kernel_event(&kev(
        2,
        &pk("bb"),
        KIND_MINT_ANNOUNCE,
        101,
        vec![
            vec!["d".to_string(), "mint-a".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
            vec!["nuts".to_string(), "11,12".to_string()],
        ],
        "",
    ));
    assert!(store.snapshot().mints.is_empty());
}

// Memoization (hot-path safety, #2880 review follow-up) lives in a child
// module split into its own file to stay under the 500-LOC hard cap; it reuses
// this module's `pk`/`kev` helpers and reads the store's private cache fields.
#[path = "mint_discovery_memoization_tests.rs"]
mod memoization;

#[test]
fn store_addressable_replace_keeps_newest_announcement() {
    let viewer = pk("aa");
    let author = pk("bb");
    let mut store = MintDiscoveryStore::new();
    store.set_viewer(Some(viewer));

    // Older announcement: not nutzap capable.
    store.ingest_kernel_event(&kev(
        2,
        &author,
        KIND_MINT_ANNOUNCE,
        100,
        vec![
            vec!["d".to_string(), "mint-a".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
            vec!["nuts".to_string(), "1,2".to_string()],
        ],
        "",
    ));
    // Newer announcement, same coordinate: now nutzap capable.
    store.ingest_kernel_event(&kev(
        3,
        &author,
        KIND_MINT_ANNOUNCE,
        200,
        vec![
            vec!["d".to_string(), "mint-a".to_string()],
            vec!["u".to_string(), "https://a.mint".to_string()],
            vec!["nuts".to_string(), "11,12".to_string()],
        ],
        "",
    ));

    let projection = store.snapshot();
    assert_eq!(projection.mints.len(), 1, "newer announcement's caps win");
    assert!(projection.mints[0].supports_nutzap);
}
