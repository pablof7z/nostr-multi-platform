use super::decode_snapshot_typed;
use crate::snapshot::ModularTimelineSnapshot;

fn nofs_projection(key: &str, id: &str) -> nmp_core::TypedProjectionData {
    let source = serde_json::json!({
        "cards": [{
            "card": {
                "id": id,
                "author_pubkey": "aa".repeat(32),
                "kind": 1,
                "created_at": 1_700_000_000_u64,
                "content": format!("feed row {id}"),
                "content_tree": { "nodes": [], "roots": [], "mode": "Plain" },
                "relation_counts": {
                    "replies": { "state": "known", "count": 0 },
                    "reactions": { "state": "known", "count": 0 },
                    "reposts": { "state": "known", "count": 0 },
                    "comments": { "state": "known", "count": 0 },
                    "zaps": { "state": "known", "count": 0 }
                }
            },
            "attribution": []
        }],
        "page": { "limit": 20, "has_more": false, "total_blocks": 1 },
        "metrics": null
    });
    let typed: nmp_nip01::OpFeedSnapshot =
        serde_json::from_value(source).expect("test value is an OP feed snapshot");
    nmp_core::TypedProjectionData {
        key: key.to_string(),
        schema_id: nmp_nip01::OP_FEED_SCHEMA_ID.to_string(),
        schema_version: nmp_nip01::OP_FEED_SCHEMA_VERSION,
        file_identifier: String::from_utf8_lossy(nmp_nip01::OP_FEED_FILE_IDENTIFIER).into_owned(),
        payload: nmp_nip01::encode_op_feed_snapshot(&typed),
        ..Default::default()
    }
}

#[test]
fn typed_op_feed_sidecars_materialize_under_each_dynamic_key() {
    let author_key = format!("nmp.feed.author.{}", "bb".repeat(32));
    let thread_key = format!("nmp.feed.thread.{}", "cc".repeat(32));
    let frame = nmp_core::encode_snapshot_frame(
        &nmp_core::SnapshotEnvelope::default(),
        &[
            nofs_projection("nmp.feed.home", "home-row"),
            nofs_projection(&author_key, "author-row"),
            nofs_projection(&thread_key, "thread-row"),
        ],
    );

    let snapshot = decode_snapshot_typed(&frame, &mut nmp_core::refs::RefProfileStore::new())
        .expect("typed frame decodes");

    let home: ModularTimelineSnapshot = snapshot
        .projection("nmp.feed.home")
        .expect("home feed is present");
    let author: ModularTimelineSnapshot = snapshot
        .projection(&author_key)
        .expect("author feed is present");
    let thread: ModularTimelineSnapshot = snapshot
        .projection(&thread_key)
        .expect("thread feed is present");

    assert_eq!(home.cards[0].card.id, "home-row");
    assert_eq!(author.cards[0].card.id, "author-row");
    assert_eq!(thread.cards[0].card.id, "thread-row");
}

/// ADR-0063 (#1671 Lane F): a `refs.profile` row-delta sidecar (the resolve_ref
/// output of a `kind:0` ingest) merges into the persistent `RefProfileStore` and
/// surfaces in `snap.refs_profiles` for avatar/name rendering. A SECOND frame
/// carrying only an incremental update mutates the same key in place — proving
/// the store persists across frames (it is NOT rebuilt per frame).
#[test]
fn refs_profile_sidecar_populates_refs_profiles_across_frames() {
    use nmp_core::refs::{encode_ref_row_delta_batch, RefRow, RefRowDeltaBatch, REFS_PROFILE_KEY};
    use nmp_core::typed_projections::{encode_profile, ProfileCardModel};

    let alice = "aa".repeat(32);

    let refs_sidecar = |baseline: bool, rev: u64, name: &str| nmp_core::TypedProjectionData {
        key: REFS_PROFILE_KEY.to_string(),
        schema_id: REFS_PROFILE_KEY.to_string(),
        schema_version: 1,
        file_identifier: "NRRD".to_string(),
        payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
            namespace: "profile".to_string(),
            baseline,
            rows: vec![RefRow::changed(
                alice.clone(),
                rev,
                encode_profile(&ProfileCardModel {
                    pubkey: alice.clone(),
                    display_name: Some(name.to_string()),
                    ..Default::default()
                }),
            )],
        }),
        ..Default::default()
    };

    // One persistent store across BOTH frames (the reader-thread lifetime model).
    let mut store = nmp_core::refs::RefProfileStore::new();

    // Frame 1: baseline resolves Alice.
    let frame1 = nmp_core::encode_snapshot_frame(
        &nmp_core::SnapshotEnvelope::default(),
        &[refs_sidecar(true, 1, "Alice")],
    );
    let snap1 = decode_snapshot_typed(&frame1, &mut store).expect("frame 1 decodes");
    assert_eq!(
        snap1.refs_profiles[&alice].display_name.as_deref(),
        Some("Alice"),
        "kind:0 ingest surfaces in refs_profiles"
    );

    // Frame 2: incremental (NOT a baseline) carrying only Alice's newer kind:0.
    let frame2 = nmp_core::encode_snapshot_frame(
        &nmp_core::SnapshotEnvelope::default(),
        &[refs_sidecar(false, 2, "Alice v2")],
    );
    let snap2 = decode_snapshot_typed(&frame2, &mut store).expect("frame 2 decodes");
    assert_eq!(
        snap2.refs_profiles[&alice].display_name.as_deref(),
        Some("Alice v2"),
        "an incremental row-delta updates the persisted store in place"
    );
}
