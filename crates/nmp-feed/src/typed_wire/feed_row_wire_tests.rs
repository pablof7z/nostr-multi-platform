use super::*;
use crate::{DeliveryMode, FeedPage, RootCard, TypedRef};

fn sample_row() -> FeedRow {
    FeedRow {
        canonical_row_id: "30023:author:article".to_string(),
        source_id: "comment-event-id".to_string(),
        author_pubkey: String::new(),
        kind: 0,
        created_at: 0,
        content: String::new(),
        tags: vec![vec!["K".to_string(), "30023".to_string()]],
        relay_provenance: vec!["wss://relay.example".to_string()],
        refs: vec![TypedRef::delivered_address(30_023, "author", "article")],
        context: vec![FeedRowContext::CommentedBy {
            author_pubkey: "commenter".to_string(),
            comment_event_id: "comment-event-id".to_string(),
            comment_created_at: 42,
        }],
    }
}

#[test]
fn constants_are_stable() {
    assert_eq!(FEED_ROW_SCHEMA_ID, "nmp.feed.feed_row");
    assert_eq!(FEED_ROW_FILE_IDENTIFIER, b"NFRS");
    assert_eq!(FEED_ROW_SCHEMA_VERSION, 1);
}

#[test]
fn empty_snapshot_round_trips() {
    let snapshot = FeedRowSnapshot {
        cards: Vec::new(),
        page: None,
        metrics: None,
    };
    let decoded = decode_feed_row_snapshot(&encode_feed_row_snapshot(&snapshot)).expect("decode");
    assert_eq!(decoded, snapshot);
}

#[test]
fn row_with_refs_and_context_round_trips() {
    let snapshot = FeedRowSnapshot {
        cards: vec![RootCard { card: sample_row() }],
        page: Some(FeedPage {
            limit: 25,
            next_cursor: None,
            has_more: false,
            total_blocks: 1,
        }),
        metrics: None,
    };
    let decoded = decode_feed_row_snapshot(&encode_feed_row_snapshot(&snapshot)).expect("decode");
    assert_eq!(decoded, snapshot);
}

#[test]
fn authored_and_reposted_by_and_group_contexts_round_trip() {
    let row = FeedRow {
        canonical_row_id: "event-id".to_string(),
        source_id: "event-id".to_string(),
        author_pubkey: "alice".to_string(),
        kind: 1,
        created_at: 100,
        content: "hello".to_string(),
        tags: Vec::new(),
        relay_provenance: Vec::new(),
        refs: vec![TypedRef::render_only_event("quoted")],
        context: vec![
            FeedRowContext::Authored,
            FeedRowContext::RepostedBy {
                author_pubkey: "bob".to_string(),
                note_created_at: 90,
            },
            FeedRowContext::Group {
                relay: "wss://groups.example".to_string(),
                id: "g1".to_string(),
            },
        ],
    };
    let snapshot = FeedRowSnapshot {
        cards: vec![RootCard { card: row }],
        page: None,
        metrics: None,
    };
    let decoded = decode_feed_row_snapshot(&encode_feed_row_snapshot(&snapshot)).expect("decode");
    assert_eq!(decoded, snapshot);
}

#[test]
fn missing_identifier_is_rejected() {
    assert!(decode_feed_row_snapshot(&[0, 1, 2, 3]).is_err());
}

#[test]
fn delivery_mode_round_trips_both_variants() {
    assert_eq!(
        TypedRef::render_only_event("x").delivery_mode,
        DeliveryMode::RenderOnly
    );
    assert_eq!(
        TypedRef::delivered_event("x").delivery_mode,
        DeliveryMode::Delivered
    );
}
