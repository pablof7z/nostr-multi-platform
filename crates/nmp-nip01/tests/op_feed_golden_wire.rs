//! Golden wire fixtures for the NOFS OP-feed typed projection (ADR-0038 T1).
//!
//! These pin the **binary** typed-FlatBuffers wire shape of
//! `nmp_feed::RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>` as
//! encoded by `nmp_nip01::encode_op_feed_snapshot`. The in-module round-trip
//! tests (`src/op_feed/typed_wire_tests.rs`) prove the codec is lossless; this
//! file additionally freezes the exact bytes so any schema drift in
//! `op_feed.fbs` (or the encoder) becomes an explicit test failure rather than a
//! silent break of the Swift / Kotlin / TUI decoders (stages T2-T4).
//!
//! It also asserts the cross-platform identity invariants
//! (`OP_FEED_FILE_IDENTIFIER`, `OP_FEED_SCHEMA_ID`, `OP_FEED_SCHEMA_VERSION`)
//! and the ADR-0037 parity property: the typed binary decode is semantically
//! equivalent to the authoritative serde `RootFeedSnapshot` projection.
//!
//! To regenerate after an intentional schema change: run this test with
//! `--nocapture`, copy the `actual op_feed_<name> hex:` line into the matching
//! `tests/fixtures/op_feed_<name>.fb.hex`, and re-run.

use nmp_feed::{FeedCursor, FeedPage, FeedWindowMetrics, RootCard, RootFeedSnapshot};
use nmp_nip01::op_feed::{
    decode_op_feed_snapshot, encode_op_feed_snapshot, OpFeedSnapshot, OP_FEED_FILE_IDENTIFIER,
    OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION,
};
use nmp_nip01::timeline_projection::{ContentRenderData, RepostAttribution};
use nmp_nip01::{
    AuthorDisplay, Nip10ReplyAttribution, NoteRelationCounts, RelationCount, RelationCountInterest,
    TimelineEventCard,
};

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

/// A plain (non-repost) thread-root card with absent display mirrors.
fn root_card() -> TimelineEventCard {
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
            replies: RelationCount::Known { count: 1 },
            reactions: RelationCount::Known { count: 0 },
            reposts: RelationCount::Known { count: 0 },
            zaps: RelationCount::Loading {
                interest: RelationCountInterest::zaps(&hex32(0x03)),
            },
        },
        author_display_name: None,
        author_picture_url: None,
        content_preview: "a thread root".to_string(),
        reposted_by: None,
    }
}

/// A card surfaced via a kind:6 repost — exercises `RepostAttribution`.
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
            zaps: RelationCount::Known { count: 0 },
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

/// The empty projection — the shape the kernel emits before any events land.
fn empty_snapshot() -> OpFeedSnapshot {
    RootFeedSnapshot {
        cards: Vec::new(),
        page: None,
        metrics: None,
    }
}

/// A representative populated snapshot: a root-with-attribution (one present /
/// one absent display mirror), a repost card with empty attribution, and a
/// paging envelope + metrics carried via the embedded NFWM window.
fn populated_snapshot() -> OpFeedSnapshot {
    RootFeedSnapshot {
        cards: vec![
            RootCard {
                card: root_card(),
                attribution: vec![attribution(0x10, true), attribution(0x11, false)],
            },
            RootCard {
                card: repost_card(),
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
fn op_feed_empty_golden_fixture_is_stable() {
    let wire = encode_op_feed_snapshot(&empty_snapshot());
    let expected = decode_hex(include_str!("fixtures/op_feed_empty_v1.fb.hex"));
    if wire != expected {
        eprintln!("actual op_feed_empty_v1 hex:\n{}", encode_hex(&wire));
    }
    assert_eq!(wire, expected, "OpFeedSnapshot empty v1 golden fixture drifted");
}

#[test]
fn op_feed_populated_golden_fixture_is_stable() {
    let wire = encode_op_feed_snapshot(&populated_snapshot());
    let expected = decode_hex(include_str!("fixtures/op_feed_populated_v1.fb.hex"));
    if wire != expected {
        eprintln!("actual op_feed_populated_v1 hex:\n{}", encode_hex(&wire));
    }
    assert_eq!(
        wire, expected,
        "OpFeedSnapshot populated v1 golden fixture drifted"
    );
}

#[test]
fn op_feed_golden_fixture_has_nofs_identifier() {
    let wire = encode_op_feed_snapshot(&populated_snapshot());
    assert_eq!(
        &wire[4..8],
        OP_FEED_FILE_IDENTIFIER,
        "buffer must carry the NOFS file identifier at bytes 4..8"
    );
    assert_eq!(OP_FEED_FILE_IDENTIFIER, b"NOFS");
}

#[test]
fn op_feed_schema_id_is_stable() {
    assert_eq!(OP_FEED_SCHEMA_ID, "nmp.nip01.opfeed");
    assert_eq!(OP_FEED_SCHEMA_VERSION, 1);
}

/// ADR-0037 acceptance criterion: parity between typed and generic. The typed
/// encoder must produce bytes that decode back to a shape semantically
/// equivalent to the authoritative serde projection.
fn assert_typed_serde_parity(snapshot: &OpFeedSnapshot) {
    let typed_bytes = encode_op_feed_snapshot(snapshot);
    let decoded = decode_op_feed_snapshot(&typed_bytes).expect("must decode");
    let via_json = serde_json::to_value(snapshot).expect("serde");
    let via_typed_json = serde_json::to_value(&decoded).expect("serde");
    assert_eq!(
        via_json, via_typed_json,
        "typed decode must be semantically equivalent to the serde projection"
    );
}

#[test]
fn op_feed_typed_serde_parity_matches_adr_0037() {
    assert_typed_serde_parity(&empty_snapshot());
    assert_typed_serde_parity(&populated_snapshot());
}
