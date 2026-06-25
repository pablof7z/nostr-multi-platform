//! Parity tests: the typed `nmp.feed.home` sidecar encodes and decodes through
//! the snapshot envelope without losing the projection's semantics (ADR-0037
//! acceptance criterion, ADR-0038 T1).
//!
//! Reshaped from NFTS (`ModularTimelineSnapshot` — the pre-V-80 blocks+cards
//! shape) to NOFS (`RootFeedSnapshot` — the OP-centric shape the producer now
//! emits). The NFTS codec itself stays (`nmp-nip01` keeps it as the future
//! thread-detail typed shape, and NOFS reuses its `TimelineEventCard` table);
//! only this feed-keyed parity wiring is rebound to the live descriptor.
//!
//! Cards are built via the production `TimelineEventCard::from_event_for_op_feed`
//! card-builder (the exact path the `OpFeedEngine` uses), so the test exercises
//! the real content-tree / render-data shape without naming `nmp-content`.

use nmp_core::substrate::KernelEvent;
use nmp_core::{decode_snapshot_typed_projections, encode_snapshot_frame, SnapshotEnvelope, TypedProjectionData};
use nmp_feed::{FeedCursor, FeedPage, RootCard, RootFeedSnapshot};
use nmp_nip01::op_feed::{
    decode_op_feed_snapshot, encode_op_feed_snapshot, OpFeedSnapshot, OP_FEED_FILE_IDENTIFIER,
    OP_FEED_SCHEMA_ID, OP_FEED_SCHEMA_VERSION,
};
use nmp_nip01::{AuthorDisplay, Nip10ReplyAttribution, TimelineEventCard};

fn hex32(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn root_event(id: u8) -> KernelEvent {
    KernelEvent {
        id: hex32(id),
        author: hex32(0x04),
        kind: 1,
        created_at: 1_700_000_500,
        tags: Vec::new(),
        content: "a thread root".to_string(),
        relay_provenance: Vec::new(),
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
    let card = TimelineEventCard::from_event_for_op_feed(&root_event(0x03), None);
    RootFeedSnapshot {
        cards: vec![RootCard {
            card,
            attribution: vec![Nip10ReplyAttribution {
                author_pubkey: hex32(0x10),
                author_display: AuthorDisplay {
                    name: None,
                    picture_url: None,
                },
                reply_event_id: hex32(0x90),
                reply_created_at: 1_700_000_900,
            }],
        }],
        page: Some(FeedPage {
            limit: 50,
            next_cursor: Some(FeedCursor {
                created_at: 1_700_000_500,
                id: hex32(0x03),
            }),
            has_more: false,
            total_blocks: 1,
        }),
        metrics: None,
    }
}

fn typed_entry(snapshot: &OpFeedSnapshot) -> (Vec<u8>, TypedProjectionData) {
    let bytes = encode_op_feed_snapshot(snapshot);
    let entry = TypedProjectionData {
        key: "nmp.feed.home".to_string(),
        schema_id: OP_FEED_SCHEMA_ID.to_string(),
        schema_version: OP_FEED_SCHEMA_VERSION,
        file_identifier: std::str::from_utf8(OP_FEED_FILE_IDENTIFIER)
            .unwrap()
            .to_string(),
        payload: bytes.clone(),
        ..Default::default()
    };
    (bytes, entry)
}

#[test]
fn empty_snapshot_carries_through_envelope() {
    let snapshot = empty_snapshot();
    let (bytes, entry) = typed_entry(&snapshot);
    let envelope = encode_snapshot_frame(
        &SnapshotEnvelope {
            rev: 1,
            ..Default::default()
        },
        &[entry],
    );
    let recovered = decode_snapshot_typed_projections(&envelope).expect("decode");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].key, "nmp.feed.home");
    assert_eq!(recovered[0].schema_id, OP_FEED_SCHEMA_ID);
    assert_eq!(recovered[0].payload, bytes);
}

#[test]
fn populated_snapshot_carries_through_envelope() {
    let snapshot = populated_snapshot();
    let (bytes, entry) = typed_entry(&snapshot);
    let envelope = encode_snapshot_frame(
        &SnapshotEnvelope {
            rev: 2,
            ..Default::default()
        },
        &[entry],
    );
    let recovered = decode_snapshot_typed_projections(&envelope).expect("decode");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].payload, bytes);
    // The recovered opaque payload decodes back to the original projection.
    let decoded = decode_op_feed_snapshot(&recovered[0].payload).expect("decode NOFS");
    assert_eq!(decoded, snapshot);
}

#[test]
fn typed_decode_round_trips_empty_snapshot() {
    let snapshot = empty_snapshot();
    let bytes = encode_op_feed_snapshot(&snapshot);
    let decoded = decode_op_feed_snapshot(&bytes).expect("must decode");
    assert_eq!(decoded.cards.len(), 0);
    assert!(decoded.page.is_none());
    assert!(decoded.metrics.is_none());
}

#[test]
fn schema_constants_match_adr_0038() {
    assert_eq!(OP_FEED_SCHEMA_ID, "nmp.nip01.opfeed");
    assert_eq!(OP_FEED_FILE_IDENTIFIER, b"NOFS");
    assert_eq!(OP_FEED_SCHEMA_VERSION, 1);
}
