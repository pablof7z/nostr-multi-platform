//! Round-trip + structural-fidelity tests for the NOFS OP-feed typed wire.
//!
//! Proves the codec is lossless against the authoritative serde
//! [`RootFeedSnapshot`] shape (ADR-0037 parity property) and that the D5
//! attribution cap is enforced at encode. The binary-golden freeze lives in
//! `tests/op_feed_golden_wire.rs`.

use nmp_feed::{FeedCursor, FeedPage, FeedWindowMetrics, RootCard, RootFeedSnapshot};

use super::*;
use crate::note_relations::{NoteRelationCounts, RelationCount, RelationCountInterest};
use crate::profile_display::AuthorDisplay;
use crate::timeline_projection::{ContentRenderData, RepostAttribution, TimelineEventCard};

/// Deterministic 32-byte hex id from a single byte (`0xab` -> "abab...ab").
fn hex32(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn content_tree() -> nmp_content::ContentTreeWire {
    use nmp_content::{tokenize_with_kind, RenderMode};
    tokenize_with_kind("hello #nostr https://example.com", &[], RenderMode::Auto, 1).to_wire()
}

fn full_display() -> AuthorDisplay {
    AuthorDisplay {
        name: Some("Alice".to_string()),
        npub: Some("npub1alice".to_string()),
        picture_url: Some("https://example.com/a.png".to_string()),
    }
}

/// A root card exercising every load-bearing path: nested content tree, both
/// `RelationCount` variants, and a `RepostAttribution`.
fn repost_card() -> TimelineEventCard {
    TimelineEventCard {
        id: hex32(0x09),
        author_pubkey: hex32(0x02),
        author_display: full_display(),
        kind: 6,
        created_at: 1_700_000_000,
        content: "hello world".to_string(),
        content_tree: content_tree(),
        content_render: ContentRenderData::default(),
        relation_counts: NoteRelationCounts {
            replies: RelationCount::Known { count: 2 },
            reactions: RelationCount::Loading {
                interest: RelationCountInterest::reactions(&hex32(0xaa)),
            },
            reposts: RelationCount::Known { count: 1 },
            zaps: RelationCount::Loading {
                interest: RelationCountInterest::zaps(&hex32(0xaa)),
            },
        },
        author_display_name: Some("Alice".to_string()),
        author_picture_url: Some("https://example.com/a.png".to_string()),
        content_preview: "hello world".to_string(),
        reposted_by: Some(RepostAttribution {
            author_pubkey: hex32(0x42),
            author_display: full_display(),
            author_display_name: Some("Alice".to_string()),
            author_picture_url: Some("https://example.com/a.png".to_string()),
            note_created_at: 1_699_000_000,
        }),
    }
}

/// A plain (non-repost) root card with absent display mirrors — proves the
/// `has_*` absence flags survive (no kind:0 seen yet).
fn bare_card() -> TimelineEventCard {
    TimelineEventCard {
        id: hex32(0x03),
        author_pubkey: hex32(0x04),
        author_display: AuthorDisplay {
            name: None,
            npub: Some("npub1bob".to_string()),
            picture_url: None,
        },
        kind: 1,
        created_at: 1_700_000_500,
        content: "a thread root".to_string(),
        content_tree: content_tree(),
        content_render: ContentRenderData::default(),
        relation_counts: NoteRelationCounts {
            replies: RelationCount::Known { count: 0 },
            reactions: RelationCount::Known { count: 0 },
            reposts: RelationCount::Known { count: 0 },
            zaps: RelationCount::Known { count: 0 },
        },
        author_display_name: None,
        author_picture_url: None,
        content_preview: "a thread root".to_string(),
        reposted_by: None,
    }
}

fn attribution(byte: u8, with_display: bool) -> Nip10ReplyAttribution {
    Nip10ReplyAttribution {
        author_pubkey: hex32(byte),
        author_display: if with_display {
            full_display()
        } else {
            AuthorDisplay {
                name: None,
                npub: Some("npub1carol".to_string()),
                picture_url: None,
            }
        },
        author_display_name: with_display.then(|| "Alice".to_string()),
        author_picture_url: with_display.then(|| "https://example.com/a.png".to_string()),
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

/// A populated snapshot: a root-with-attribution (bare card) + a repost card
/// (empty attribution) + a paging envelope. This is the representative shape
/// ADR-0038 §golden-wire calls for.
fn populated_snapshot() -> RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution> {
    RootFeedSnapshot {
        cards: vec![
            RootCard {
                card: bare_card(),
                attribution: vec![attribution(0x10, true), attribution(0x11, false)],
            },
            RootCard {
                card: repost_card(),
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
fn schema_constants_match_adr_0038() {
    assert_eq!(OP_FEED_SCHEMA_ID, "nmp.nip01.opfeed");
    assert_eq!(OP_FEED_FILE_IDENTIFIER, b"NOFS");
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
        "empty buffer must carry the NOFS identifier"
    );
    let decoded = decode_op_feed_snapshot(&bytes).expect("decode empty");
    assert_eq!(decoded, snapshot);
    assert!(decoded.cards.is_empty());
    assert!(decoded.page.is_none());
    assert!(decoded.metrics.is_none());
}

#[test]
fn populated_snapshot_round_trips() {
    let snapshot = populated_snapshot();
    let bytes = encode_op_feed_snapshot(&snapshot);
    let decoded = decode_op_feed_snapshot(&bytes).expect("decode populated");
    assert_eq!(decoded, snapshot, "full round-trip must be byte-faithful");
}

#[test]
fn root_with_attribution_preserves_raw_fields() {
    let snapshot = populated_snapshot();
    let decoded = decode_op_feed_snapshot(&encode_op_feed_snapshot(&snapshot)).expect("decode");

    let root = &decoded.cards[0];
    assert_eq!(root.card.id, hex32(0x03));
    assert_eq!(root.attribution.len(), 2);
    // Raw hex pubkey + raw reply event id survive verbatim.
    assert_eq!(root.attribution[0].author_pubkey, hex32(0x10));
    assert_eq!(root.attribution[0].reply_event_id, hex32(0x90));
    assert_eq!(root.attribution[0].reply_created_at, 1_700_000_900 + 0x10);
    // Present display mirrors survive.
    assert_eq!(
        root.attribution[0].author_display_name.as_deref(),
        Some("Alice")
    );
    // Absent display mirrors stay `None` (has_* = false, no kind:0 yet).
    assert_eq!(root.attribution[1].author_display_name, None);
    assert_eq!(root.attribution[1].author_picture_url, None);
}

#[test]
fn repost_card_and_embedded_window_survive() {
    let snapshot = populated_snapshot();
    let decoded = decode_op_feed_snapshot(&encode_op_feed_snapshot(&snapshot)).expect("decode");

    // The repost card (empty attribution) round-trips, including its
    // RepostAttribution and the embedded NFCT content tree carried inside the
    // reused TimelineEventCard encoder.
    let repost = &decoded.cards[1];
    assert!(repost.attribution.is_empty());
    assert!(repost.card.reposted_by.is_some());
    assert_eq!(repost.card.kind, 6);

    // The embedded NFWM feed-window sub-buffer carries page + metrics.
    let recovered_page = decoded.page.expect("page present");
    assert_eq!(recovered_page.limit, 50);
    assert!(recovered_page.has_more);
    assert_eq!(recovered_page.total_blocks, 2);
    assert_eq!(decoded.metrics.expect("metrics present").make_window_us, 1234);
}

#[test]
fn attribution_is_bounded_at_encode() {
    // Build a root whose attribution vector exceeds the D5 cap; the encoder must
    // truncate to MAX_ATTRIBUTION_PER_ROOT.
    let over = nmp_feed::MAX_ATTRIBUTION_PER_ROOT + 5;
    let attribution: Vec<Nip10ReplyAttribution> = (0..over)
        .map(|i| attribution((i % 200) as u8, false))
        .collect();
    let snapshot = RootFeedSnapshot {
        cards: vec![RootCard {
            card: bare_card(),
            attribution,
        }],
        page: None,
        metrics: None,
    };
    let decoded = decode_op_feed_snapshot(&encode_op_feed_snapshot(&snapshot)).expect("decode");
    assert_eq!(
        decoded.cards[0].attribution.len(),
        nmp_feed::MAX_ATTRIBUTION_PER_ROOT,
        "encoder must bound attribution at MAX_ATTRIBUTION_PER_ROOT (D5)"
    );
}

#[test]
fn decode_rejects_non_nofs_buffer() {
    assert!(decode_op_feed_snapshot(&[]).is_err());
    assert!(decode_op_feed_snapshot(b"not a flatbuffer").is_err());
}

/// ADR-0037 parity property: the typed binary decode is semantically equivalent
/// to the authoritative serde `RootFeedSnapshot` — re-serializing the decoded
/// value yields the same JSON the generic `Value` fallback path emits.
#[test]
fn typed_decode_matches_serde_projection() {
    let snapshot = populated_snapshot();
    let decoded = decode_op_feed_snapshot(&encode_op_feed_snapshot(&snapshot)).expect("decode");
    let typed_json = serde_json::to_value(&decoded).expect("typed to json");
    let serde_json_value = serde_json::to_value(&snapshot).expect("serde to json");
    assert_eq!(
        typed_json, serde_json_value,
        "typed decode must equal the generic Value projection"
    );
}
