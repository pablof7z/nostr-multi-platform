use nmp_content::ContentTreeWire;
use nmp_threading::{ThreadPointer, TimelineBlock};

use super::*;
use crate::note_relations::{NoteRelationCounts, RelationCount, RelationCountInterest};
use crate::profile_display::AuthorDisplay;
use crate::timeline_projection::{
    ContentEventRenderData, ContentProfileRenderData, ContentRenderData, ModularTimelineSnapshot,
    RepostAttribution, TimelineEventCard, TimelineWindowCursor, TimelineWindowMetrics,
    TimelineWindowPage,
};

fn event_id(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn known_counts() -> NoteRelationCounts {
    NoteRelationCounts {
        replies: RelationCount::Known { count: 3 },
        reactions: RelationCount::Known { count: 5 },
        reposts: RelationCount::Known { count: 1 },
        zaps: RelationCount::Known { count: 7 },
    }
}

fn loading_counts() -> NoteRelationCounts {
    NoteRelationCounts {
        replies: RelationCount::Known { count: 2 },
        reactions: RelationCount::Loading {
            interest: RelationCountInterest::reactions(&event_id(0xaa)),
        },
        reposts: RelationCount::Loading {
            interest: RelationCountInterest::reposts(&event_id(0xaa)),
        },
        zaps: RelationCount::Loading {
            interest: RelationCountInterest::zaps(&event_id(0xaa)),
        },
    }
}

fn full_author_display() -> AuthorDisplay {
    AuthorDisplay {
        name: Some("Alice".to_string()),
        npub: Some("npub1alice".to_string()),
        picture_url: Some("https://example.com/a.png".to_string()),
    }
}

fn fallback_author_display() -> AuthorDisplay {
    AuthorDisplay {
        name: None,
        npub: Some("npub1bob".to_string()),
        picture_url: None,
    }
}

fn rich_content_tree() -> ContentTreeWire {
    use nmp_content::{tokenize_with_kind, RenderMode};
    tokenize_with_kind("hello #nostr https://example.com", &[], RenderMode::Auto, 1).to_wire()
}

fn rich_content_render() -> ContentRenderData {
    let mut render = ContentRenderData::default();
    render.profiles.insert(
        event_id(0x11),
        ContentProfileRenderData {
            pubkey: event_id(0x11),
            display: full_author_display(),
        },
    );
    render.events.insert(
        event_id(0x22),
        ContentEventRenderData {
            id: event_id(0x22),
            author_pubkey: event_id(0x33),
            author_display: fallback_author_display(),
            kind: 1,
            created_at: 1_700_000_100,
            content_preview: "embedded preview".to_string(),
            content_tree: rich_content_tree(),
        },
    );
    render
}

fn sample_card() -> TimelineEventCard {
    TimelineEventCard {
        id: event_id(0x01),
        author_pubkey: event_id(0x02),
        author_display: full_author_display(),
        kind: 1,
        created_at: 1_700_000_000,
        content: "hello world".to_string(),
        content_tree: rich_content_tree(),
        content_render: rich_content_render(),
        relation_counts: known_counts(),
        author_display_name: Some("Alice".to_string()),
        author_picture_url: Some("https://example.com/a.png".to_string()),
        content_preview: "hello world".to_string(),
        reposted_by: None,
    }
}

fn repost_card() -> TimelineEventCard {
    let mut card = sample_card();
    card.id = event_id(0x09);
    card.kind = 6;
    card.author_display = fallback_author_display();
    card.author_display_name = None;
    card.author_picture_url = None;
    card.relation_counts = loading_counts();
    card.reposted_by = Some(RepostAttribution {
        author_pubkey: event_id(0x42),
        author_display: full_author_display(),
        author_display_name: Some("Alice".to_string()),
        author_picture_url: Some("https://example.com/a.png".to_string()),
        note_created_at: 1_699_000_000,
    });
    card
}

fn sample_page() -> TimelineWindowPage {
    TimelineWindowPage {
        limit: 80,
        next_cursor: Some(TimelineWindowCursor {
            created_at: 1_699_999_000,
            id: event_id(0x05),
        }),
        has_more: true,
        total_blocks: 3,
    }
}

fn assert_round_trip(snapshot: &ModularTimelineSnapshot) {
    let encoded = encode_modular_timeline_snapshot(snapshot);
    assert!(!encoded.is_empty(), "encoded buffer must not be empty");
    let decoded = decode_modular_timeline_snapshot(&encoded).expect("must decode");
    assert_eq!(&decoded, snapshot, "typed wire must round-trip losslessly");
}

#[test]
fn empty_snapshot_round_trips() {
    assert_round_trip(&ModularTimelineSnapshot::empty());
}

#[test]
fn standalone_block_round_trips() {
    let snapshot = ModularTimelineSnapshot {
        blocks: vec![
            TimelineBlock::Standalone {
                id: event_id(0x01),
                root: None,
            },
            TimelineBlock::Standalone {
                id: event_id(0x02),
                root: Some(ThreadPointer::Event {
                    id: event_id(0x03),
                    relay: Some("wss://relay.example".to_string()),
                    kind: Some(1),
                }),
            },
        ],
        cards: Vec::new(),
        page: None,
        metrics: None,
    };
    assert_round_trip(&snapshot);
}

#[test]
fn module_block_round_trips() {
    let snapshot = ModularTimelineSnapshot {
        blocks: vec![TimelineBlock::Module {
            events: vec![event_id(0x10), event_id(0x11), event_id(0x12)],
            has_gap: true,
            root: Some(ThreadPointer::Address {
                coord: "30023:abcd:my-d-tag".to_string(),
                relay: None,
                kind: Some(30023),
            }),
        }],
        cards: Vec::new(),
        page: None,
        metrics: None,
    };
    assert_round_trip(&snapshot);
}

#[test]
fn external_thread_pointer_round_trips() {
    let snapshot = ModularTimelineSnapshot {
        blocks: vec![TimelineBlock::Standalone {
            id: event_id(0x01),
            root: Some(ThreadPointer::External {
                uri: "https://example.com/thread".to_string(),
            }),
        }],
        cards: Vec::new(),
        page: None,
        metrics: None,
    };
    assert_round_trip(&snapshot);
}

#[test]
fn thread_pointer_without_kind_round_trips() {
    // `kind: None` must NOT decode to `Some(0)`.
    let snapshot = ModularTimelineSnapshot {
        blocks: vec![TimelineBlock::Standalone {
            id: event_id(0x01),
            root: Some(ThreadPointer::Event {
                id: event_id(0x03),
                relay: None,
                kind: None,
            }),
        }],
        cards: Vec::new(),
        page: None,
        metrics: None,
    };
    assert_round_trip(&snapshot);

    let decoded =
        decode_modular_timeline_snapshot(&encode_modular_timeline_snapshot(&snapshot)).unwrap();
    if let TimelineBlock::Standalone {
        root: Some(ThreadPointer::Event { kind, .. }),
        ..
    } = &decoded.blocks[0]
    {
        assert_eq!(*kind, None, "absent kind must stay None, not Some(0)");
    } else {
        panic!("expected Standalone/Event pointer");
    }
}

#[test]
fn card_with_relation_counts_round_trips() {
    let snapshot = ModularTimelineSnapshot {
        blocks: Vec::new(),
        cards: vec![sample_card()],
        page: None,
        metrics: None,
    };
    assert_round_trip(&snapshot);
}

#[test]
fn card_with_loading_relation_counts_round_trips() {
    let mut card = sample_card();
    card.relation_counts = loading_counts();
    let snapshot = ModularTimelineSnapshot {
        blocks: Vec::new(),
        cards: vec![card],
        page: None,
        metrics: None,
    };
    assert_round_trip(&snapshot);
}

#[test]
fn card_with_repost_round_trips() {
    let snapshot = ModularTimelineSnapshot {
        blocks: Vec::new(),
        cards: vec![repost_card()],
        page: None,
        metrics: None,
    };
    assert_round_trip(&snapshot);
}

#[test]
fn card_absent_display_fields_stay_none() {
    let mut card = sample_card();
    card.author_display = fallback_author_display();
    card.author_display_name = None;
    card.author_picture_url = None;
    let snapshot = ModularTimelineSnapshot {
        blocks: Vec::new(),
        cards: vec![card],
        page: None,
        metrics: None,
    };
    assert_round_trip(&snapshot);

    let decoded =
        decode_modular_timeline_snapshot(&encode_modular_timeline_snapshot(&snapshot)).unwrap();
    let card = &decoded.cards[0];
    assert_eq!(card.author_display_name, None);
    assert_eq!(card.author_picture_url, None);
    assert_eq!(card.author_display.name, None);
    assert_eq!(card.author_display.picture_url, None);
}

#[test]
fn page_and_metrics_round_trip() {
    let snapshot = ModularTimelineSnapshot {
        blocks: Vec::new(),
        cards: Vec::new(),
        page: Some(sample_page()),
        metrics: Some(TimelineWindowMetrics {
            make_window_us: 1_234,
        }),
    };
    assert_round_trip(&snapshot);
}

#[test]
fn full_snapshot_round_trips() {
    let snapshot = ModularTimelineSnapshot {
        blocks: vec![
            TimelineBlock::Standalone {
                id: event_id(0x01),
                root: None,
            },
            TimelineBlock::Module {
                events: vec![event_id(0x10), event_id(0x11)],
                has_gap: false,
                root: Some(ThreadPointer::Event {
                    id: event_id(0x12),
                    relay: Some("wss://relay.example".to_string()),
                    kind: Some(1),
                }),
            },
        ],
        cards: vec![sample_card(), repost_card()],
        page: Some(sample_page()),
        metrics: Some(TimelineWindowMetrics {
            make_window_us: 999,
        }),
    };
    assert_round_trip(&snapshot);
}

#[test]
fn file_identifier_is_nfts() {
    let encoded = encode_modular_timeline_snapshot(&ModularTimelineSnapshot::empty());
    assert!(
        fb::modular_timeline_snapshot_buffer_has_identifier(&encoded),
        "buffer must carry the NFTS identifier"
    );
    // The identifier lives at bytes 4..8 of a finished FlatBuffer.
    assert_eq!(&encoded[4..8], FILE_IDENTIFIER);
    assert_eq!(FILE_IDENTIFIER, b"NFTS");
}

#[test]
fn decode_rejects_buffer_without_identifier() {
    let err = decode_modular_timeline_snapshot(&[0u8; 16]).expect_err("must reject");
    assert!(err.contains("NFTS"), "error names the missing id: {err}");
}

#[test]
fn schema_constants_are_stable() {
    assert_eq!(SCHEMA_ID, "nmp.nip01.timeline");
    assert_eq!(FILE_IDENTIFIER, b"NFTS");
    assert_eq!(SCHEMA_VERSION, 1);
}
