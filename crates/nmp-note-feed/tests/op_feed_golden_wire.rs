//! Golden wire fixtures for the NNFS OP-feed typed projection.

use nmp_feed::{FeedCursor, FeedPage, FeedWindowMetrics, RootCard, RootFeedSnapshot};
use nmp_note_feed::op_feed::{
    decode_op_feed_snapshot, encode_op_feed_snapshot, OpFeedSnapshot, OP_FEED_FILE_IDENTIFIER,
    OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION,
};
use nmp_note_feed::{Nip10ReplyAttribution, NoteFeedItem, RepostAttribution};

fn hex32(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn content_tree() -> nmp_content::ContentTreeWire {
    use nmp_content::{tokenize_with_kind, RenderMode};
    tokenize_with_kind("hello #nostr https://example.com", &[], RenderMode::Auto, 1).to_wire()
}

fn root_item() -> NoteFeedItem {
    NoteFeedItem {
        id: hex32(0x03),
        author_pubkey: hex32(0x04),
        kind: 1,
        created_at: 1_700_000_500,
        content: "a thread root".to_string(),
        content_tree: content_tree(),
        reposted_by: None,
        relay_provenance: Vec::new(),
        hosted_group: None,
    }
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
        relay_provenance: Vec::new(),
        hosted_group: None,
    }
}

fn attribution(byte: u8) -> Nip10ReplyAttribution {
    Nip10ReplyAttribution {
        author_pubkey: hex32(byte),
        reply_event_id: hex32(byte.wrapping_add(0x80)),
        reply_created_at: 1_700_000_900 + u64::from(byte),
    }
}

fn empty_snapshot() -> OpFeedSnapshot {
    RootFeedSnapshot {
        cards: Vec::new(),
        page: None,
        metrics: None,
    }
}

fn populated_snapshot() -> OpFeedSnapshot {
    RootFeedSnapshot {
        cards: vec![
            RootCard {
                card: root_item(),
                attribution: vec![attribution(0x10), attribution(0x11)],
            },
            RootCard {
                card: repost_item(),
                attribution: Vec::new(),
            },
        ],
        page: Some(FeedPage {
            limit: 50,
            next_cursor: Some(FeedCursor {
                created_at: 1_700_000_000,
                id: hex32(0x09),
            }),
            has_more: true,
            total_blocks: 2,
        }),
        metrics: Some(FeedWindowMetrics {
            make_window_us: 1234,
        }),
    }
}

fn decode_hex(hex: &str) -> Vec<u8> {
    let compact: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(compact.len() % 2, 0, "hex fixture must contain full bytes");
    compact
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("fixture is ascii hex");
            u8::from_str_radix(pair, 16).expect("fixture is valid hex")
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn op_feed_empty_v2_golden_fixture_is_stable() {
    let wire = encode_op_feed_snapshot(&empty_snapshot());
    let expected = decode_hex(include_str!("fixtures/op_feed_empty_v2.fb.hex"));
    if wire != expected {
        eprintln!("actual op_feed_empty_v2 hex:\n{}", encode_hex(&wire));
    }
    assert_eq!(wire, expected);
}

#[test]
fn op_feed_populated_v2_golden_fixture_is_stable() {
    let wire = encode_op_feed_snapshot(&populated_snapshot());
    let expected = decode_hex(include_str!("fixtures/op_feed_populated_v2.fb.hex"));
    if wire != expected {
        eprintln!("actual op_feed_populated_v2 hex:\n{}", encode_hex(&wire));
    }
    assert_eq!(wire, expected);
}

#[test]
fn op_feed_golden_fixture_has_nnfs_identifier() {
    let wire = encode_op_feed_snapshot(&populated_snapshot());
    assert_eq!(&wire[4..8], OP_FEED_FILE_IDENTIFIER);
    assert_eq!(OP_FEED_FILE_IDENTIFIER, b"NNFS");
}

#[test]
fn op_feed_schema_id_is_stable() {
    assert_eq!(OP_FEED_SCHEMA_ID, "nmp.note_feed.opfeed");
    assert_eq!(OP_FEED_SCHEMA_VERSION, 2);
}

#[test]
fn op_feed_typed_serde_parity_matches_projection() {
    let snapshot = populated_snapshot();
    let typed_bytes = encode_op_feed_snapshot(&snapshot);
    let decoded = decode_op_feed_snapshot(&typed_bytes).expect("must decode");
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap(),
        serde_json::to_value(&decoded).unwrap()
    );
}
