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

    let snapshot = decode_snapshot_typed(&frame).expect("typed frame decodes");

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
