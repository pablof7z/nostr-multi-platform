//! Round-trip + structural-fidelity tests for the NNFS OP-feed typed wire.

use nmp_feed::{FeedCursor, FeedPage, FeedWindowMetrics, RootCard, RootFeedSnapshot};

use super::*;
use crate::{NoteFeedItem, RepostAttribution};

fn hex32(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn content_tree() -> nmp_content::ContentTreeWire {
    use nmp_content::{tokenize_with_kind, RenderMode};
    tokenize_with_kind("hello #nostr https://example.com", &[], RenderMode::Auto, 1).to_wire()
}

fn repost_item() -> NoteFeedItem {
    NoteFeedItem {
        id: hex32(0x09),
        author_pubkey: hex32(0x02),
        kind: 6,
        created_at: 1_700_000_000,
        content: "hello world".to_string(),
        content_tree: content_tree(),
        reposted_by: Some(RepostAttribution {
            author_pubkey: hex32(0x42),
            note_created_at: 1_699_000_000,
        }),
        relay_provenance: vec!["wss://relay.example".to_string()],
    }
}

fn bare_item() -> NoteFeedItem {
    NoteFeedItem {
        id: hex32(0x03),
        author_pubkey: hex32(0x04),
        kind: 1,
        created_at: 1_700_000_500,
        content: "a thread root".to_string(),
        content_tree: content_tree(),
        reposted_by: None,
        relay_provenance: Vec::new(),
    }
}

fn attribution(byte: u8) -> Nip10ReplyAttribution {
    Nip10ReplyAttribution {
        author_pubkey: hex32(byte),
        reply_event_id: hex32(byte.wrapping_add(0x80)),
        reply_created_at: 1_700_000_900 + u64::from(byte),
    }
}

fn page() -> FeedPage {
    FeedPage {
        limit: 50,
        next_cursor: Some(FeedCursor {
            created_at: 1_700_000_000,
            id: hex32(0x09),
        }),
        has_more: true,
        total_blocks: 2,
    }
}

fn populated_snapshot() -> RootFeedSnapshot<NoteFeedItem, Nip10ReplyAttribution> {
    RootFeedSnapshot {
        cards: vec![
            RootCard {
                card: bare_item(),
                attribution: vec![attribution(0x10), attribution(0x11)],
            },
            RootCard {
                card: repost_item(),
                attribution: Vec::new(),
            },
        ],
        page: Some(page()),
        metrics: Some(FeedWindowMetrics {
            make_window_us: 1234,
        }),
    }
}

#[test]
fn schema_constants_match_note_feed_owner() {
    assert_eq!(OP_FEED_SCHEMA_ID, "nmp.note_feed.opfeed");
    assert_eq!(OP_FEED_FILE_IDENTIFIER, b"NNFS");
    assert_eq!(OP_FEED_SCHEMA_VERSION, 1);
}

#[test]
fn empty_snapshot_round_trips() {
    let snapshot = RootFeedSnapshot {
        cards: Vec::new(),
        page: None,
        metrics: None,
    };
    let bytes = encode_op_feed_snapshot(&snapshot);
    assert!(
        fb::op_feed_snapshot_buffer_has_identifier(&bytes),
        "empty buffer must carry the NNFS identifier"
    );
    let decoded = decode_op_feed_snapshot(&bytes).expect("decode empty");
    assert_eq!(decoded, snapshot);
}

#[test]
fn populated_snapshot_round_trips() {
    let snapshot = populated_snapshot();
    let bytes = encode_op_feed_snapshot(&snapshot);
    let decoded = decode_op_feed_snapshot(&bytes).expect("decode populated");
    assert_eq!(decoded, snapshot);
}

#[test]
fn root_with_attribution_preserves_raw_fields() {
    let decoded =
        decode_op_feed_snapshot(&encode_op_feed_snapshot(&populated_snapshot())).expect("decode");

    let root = &decoded.cards[0];
    assert_eq!(root.card.id, hex32(0x03));
    assert_eq!(root.card.author_pubkey, hex32(0x04));
    assert_eq!(root.attribution.len(), 2);
    assert_eq!(root.attribution[0].author_pubkey, hex32(0x10));
    assert_eq!(root.attribution[0].reply_event_id, hex32(0x90));
    assert_eq!(root.attribution[0].reply_created_at, 1_700_000_900 + 0x10);
}

#[test]
fn repost_item_and_embedded_window_survive() {
    let decoded =
        decode_op_feed_snapshot(&encode_op_feed_snapshot(&populated_snapshot())).expect("decode");

    let repost = &decoded.cards[1];
    assert!(repost.attribution.is_empty());
    assert!(repost.card.reposted_by.is_some());
    assert_eq!(repost.card.kind, 6);
    assert_eq!(repost.card.relay_provenance, vec!["wss://relay.example"]);

    let recovered_page = decoded.page.expect("page present");
    assert_eq!(recovered_page.limit, 50);
    assert!(recovered_page.has_more);
    assert_eq!(recovered_page.total_blocks, 2);
    assert_eq!(
        decoded.metrics.expect("metrics present").make_window_us,
        1234
    );
}

#[test]
fn attribution_is_bounded_at_encode() {
    let over = nmp_feed::MAX_ATTRIBUTION_PER_ROOT + 5;
    let attribution: Vec<Nip10ReplyAttribution> =
        (0..over).map(|i| attribution((i % 200) as u8)).collect();
    let snapshot = RootFeedSnapshot {
        cards: vec![RootCard {
            card: bare_item(),
            attribution,
        }],
        page: None,
        metrics: None,
    };
    let decoded = decode_op_feed_snapshot(&encode_op_feed_snapshot(&snapshot)).expect("decode");
    assert_eq!(
        decoded.cards[0].attribution.len(),
        nmp_feed::MAX_ATTRIBUTION_PER_ROOT
    );
}

#[test]
fn decode_rejects_non_nnfs_buffer() {
    assert!(decode_op_feed_snapshot(&[]).is_err());
    assert!(decode_op_feed_snapshot(b"not a flatbuffer").is_err());
}

#[test]
fn typed_decode_matches_serde_projection() {
    let snapshot = populated_snapshot();
    let decoded = decode_op_feed_snapshot(&encode_op_feed_snapshot(&snapshot)).expect("decode");
    let typed_json = serde_json::to_value(&decoded).expect("typed to json");
    let serde_json_value = serde_json::to_value(&snapshot).expect("serde to json");
    assert_eq!(typed_json, serde_json_value);
}
