use super::*;
use crate::bridge::{NmpEvent, UpdatePayload};

const HOME_AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROFILE_AUTHOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const THREAD_ROOT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn nofs_projection(key: &str, id: &str, author: &str) -> nmp_core::TypedProjectionData {
    let source = serde_json::json!({
        "cards": [{
            "card": {
                "id": id,
                "author_pubkey": author,
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

fn event_with_feeds(feeds: &[nmp_core::TypedProjectionData]) -> NmpEvent {
    NmpEvent {
        payload: UpdatePayload::FlatBuffers(nmp_core::encode_snapshot_frame(
            &nmp_core::SnapshotEnvelope::default(),
            feeds,
        )),
    }
}

#[test]
fn profile_rows_come_from_author_feed_not_home_feed() {
    let (runtime, _rx) = AppRuntime::new().expect("runtime starts");
    let mut state = AppState {
        profile_pubkey: PROFILE_AUTHOR.to_string(),
        ..Default::default()
    };
    let author_key = format!("nmp.feed.author.{PROFILE_AUTHOR}");
    let event = event_with_feeds(&[
        nofs_projection("nmp.feed.home", "home-row", HOME_AUTHOR),
        nofs_projection(&author_key, "author-row", PROFILE_AUTHOR),
    ]);

    state.apply_nmp_event(&runtime, event);

    assert_eq!(state.rows[0].id, "home-row");
    assert_eq!(state.rows[0].author_pubkey, HOME_AUTHOR);
    assert_eq!(state.profile_rows[0].id, "author-row");
    assert_eq!(state.profile_rows[0].author_pubkey, PROFILE_AUTHOR);
}

#[test]
fn thread_rows_come_from_thread_feed_not_home_feed() {
    let (runtime, _rx) = AppRuntime::new().expect("runtime starts");
    let mut state = AppState {
        thread_event_id: THREAD_ROOT.to_string(),
        detail_cursor: 99,
        ..Default::default()
    };
    let thread_key = format!("nmp.feed.thread.{THREAD_ROOT}");
    let event = event_with_feeds(&[
        nofs_projection("nmp.feed.home", "home-row", HOME_AUTHOR),
        nofs_projection(&thread_key, "thread-row", PROFILE_AUTHOR),
    ]);

    state.apply_nmp_event(&runtime, event);

    assert_eq!(state.rows[0].id, "home-row");
    assert_eq!(state.thread_rows[0].id, "thread-row");
    assert_eq!(state.detail_cursor, 0);
}
