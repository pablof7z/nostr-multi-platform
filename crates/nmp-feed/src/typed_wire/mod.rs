//! Typed FlatBuffers wire encoding for the `nmp.feed.home` projection.
//!
//! The home feed snapshot is a binary `nmp.feed.home.HomeFeedSnapshot` value
//! tree with file identifier `NFHF`. Unlike the generic JSON-shaped
//! `nmp.transport.UpdateFrame` (see `nmp_core::update_envelope`), this is a
//! *typed* projection: every field is a concrete FlatBuffers slot, so hosts
//! decode fields by name instead of walking an untyped value tree.
//!
//! ## Doctrine: raw bytes only on the wire
//!
//! All pubkeys and event ids are raw 32-byte vectors (`[ubyte]` in the
//! schema). No `display::` helpers, no npub/bech32 encoding, no profile names.
//! Display formatting is a host concern; the wire carries raw identity bytes.
//!
//! The checked-in bindings in `generated/feed_home_generated.rs` are produced
//! by `flatc` from `schema/feed_home.fbs`. Regenerate only with the workspace
//! FlatBuffers pin (`25.12.19`):
//!
//! ```sh
//! flatc --rust -o crates/nmp-feed/src/typed_wire/generated \
//!       crates/nmp-feed/schema/feed_home.fbs
//! ```

#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "generated/feed_home_generated.rs"]
mod feed_home_generated;

use feed_home_generated::nmp::feed::home as fb;
use flatbuffers::{FlatBufferBuilder, WIPOffset};

/// Stable projection identifier this wire shape projects into.
pub const SCHEMA_ID: &str = "nmp.feed.home";

/// FlatBuffers file identifier for a `HomeFeedSnapshot` root buffer.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NFHF";

/// Schema version of the typed home-feed payload. Bump on any breaking
/// field change. Mirrors `HomeFeedSnapshot.schema_version` in the `.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

/// A raw-pubkey-only snapshot of the home feed, suitable for typed
/// FlatBuffers wire encoding. All display helpers are forbidden here
/// (doctrine: raw data only on the wire).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HomeFeedWire {
    pub pages: Vec<FeedPageWire>,
    pub window_limit: u32,
    pub window_offset: u32,
    pub total_items: u32,
}

/// A page of timeline cards at a specific cursor position.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FeedPageWire {
    pub cards: Vec<EventCardWire>,
    pub cursor_oldest_created_at: i64,
    pub cursor_oldest_event_id: Option<[u8; 32]>,
    pub is_complete: bool,
}

/// A single timeline event card: the fundamental unit of the home feed.
///
/// Relation counts and repost attribution are flattened here for an
/// ergonomic Rust API; the encoder reconstructs the nested
/// `RelationCounts` / `RepostAttribution` FlatBuffers tables on the wire.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventCardWire {
    pub event_id: [u8; 32],
    pub pubkey: [u8; 32],
    pub created_at: i64,
    pub kind: u32,
    pub content_json: String,
    pub reply_count: u32,
    pub reaction_count: u32,
    pub repost_count: u32,
    pub zap_msats: u64,
    pub root_event_id: Option<[u8; 32]>,
    pub repost_original_pubkey: Option<[u8; 32]>,
    pub repost_original_event_id: Option<[u8; 32]>,
}

/// Encode a home-feed snapshot as one typed FlatBuffers `HomeFeedSnapshot`
/// buffer with the `NFHF` file identifier.
#[must_use]
pub fn encode_home_feed(snapshot: &HomeFeedWire) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();

    let pages: Vec<WIPOffset<fb::FeedPage<'_>>> = snapshot
        .pages
        .iter()
        .map(|page| encode_page(&mut builder, page))
        .collect();
    let pages = builder.create_vector(&pages);

    let root = fb::HomeFeedSnapshot::create(
        &mut builder,
        &fb::HomeFeedSnapshotArgs {
            schema_version: SCHEMA_VERSION,
            pages: Some(pages),
            window_limit: snapshot.window_limit,
            window_offset: snapshot.window_offset,
            total_items: snapshot.total_items,
        },
    );
    fb::finish_home_feed_snapshot_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

fn encode_page<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    page: &FeedPageWire,
) -> WIPOffset<fb::FeedPage<'bldr>> {
    let cards: Vec<WIPOffset<fb::TimelineEventCard<'_>>> = page
        .cards
        .iter()
        .map(|card| encode_card(builder, card))
        .collect();
    let cards = builder.create_vector(&cards);
    let cursor_oldest_event_id = page
        .cursor_oldest_event_id
        .as_ref()
        .map(|id| builder.create_vector(id));

    fb::FeedPage::create(
        builder,
        &fb::FeedPageArgs {
            cards: Some(cards),
            cursor_oldest_created_at: page.cursor_oldest_created_at,
            cursor_oldest_event_id,
            is_complete: page.is_complete,
        },
    )
}

fn encode_card<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    card: &EventCardWire,
) -> WIPOffset<fb::TimelineEventCard<'bldr>> {
    // Strings/vectors must be created before the parent table.
    let content_json = builder.create_string(&card.content_json);
    let event_id = builder.create_vector(&card.event_id);
    let pubkey = builder.create_vector(&card.pubkey);
    let root_event_id = card
        .root_event_id
        .as_ref()
        .map(|id| builder.create_vector(id));

    let relation_counts = fb::RelationCounts::create(
        builder,
        &fb::RelationCountsArgs {
            replies: card.reply_count,
            reactions: card.reaction_count,
            reposts: card.repost_count,
            zap_msats: card.zap_msats,
        },
    );

    // A card is a repost only if an original-author pubkey is present.
    let repost_attribution = card.repost_original_pubkey.as_ref().map(|original_pubkey| {
        let original_author_pubkey = builder.create_vector(original_pubkey);
        let original_event_id = card
            .repost_original_event_id
            .as_ref()
            .map(|id| builder.create_vector(id));
        fb::RepostAttribution::create(
            builder,
            &fb::RepostAttributionArgs {
                original_author_pubkey: Some(original_author_pubkey),
                original_event_id,
            },
        )
    });

    fb::TimelineEventCard::create(
        builder,
        &fb::TimelineEventCardArgs {
            event_id: Some(event_id),
            pubkey: Some(pubkey),
            created_at: card.created_at,
            kind: card.kind,
            content_json: Some(content_json),
            relation_counts: Some(relation_counts),
            repost_attribution,
            root_event_id,
        },
    )
}

/// Decode a typed FlatBuffers `HomeFeedSnapshot` buffer back into the owned
/// [`HomeFeedWire`] view. Returns a human-readable error string on any
/// malformed-buffer or missing-required-field condition.
pub fn decode_home_feed(bytes: &[u8]) -> Result<HomeFeedWire, String> {
    if !fb::home_feed_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NFHF file identifier".to_string());
    }
    let snapshot = fb::root_as_home_feed_snapshot(bytes).map_err(|err| format!("{err:?}"))?;

    let mut pages = Vec::new();
    if let Some(fb_pages) = snapshot.pages() {
        pages.reserve(fb_pages.len());
        for index in 0..fb_pages.len() {
            pages.push(decode_page(fb_pages.get(index))?);
        }
    }

    Ok(HomeFeedWire {
        pages,
        window_limit: snapshot.window_limit(),
        window_offset: snapshot.window_offset(),
        total_items: snapshot.total_items(),
    })
}

fn decode_page(page: fb::FeedPage<'_>) -> Result<FeedPageWire, String> {
    let mut cards = Vec::new();
    if let Some(fb_cards) = page.cards() {
        cards.reserve(fb_cards.len());
        for index in 0..fb_cards.len() {
            cards.push(decode_card(fb_cards.get(index))?);
        }
    }
    Ok(FeedPageWire {
        cards,
        cursor_oldest_created_at: page.cursor_oldest_created_at(),
        cursor_oldest_event_id: page
            .cursor_oldest_event_id()
            .map(|v| array_32(v.bytes(), "cursor_oldest_event_id"))
            .transpose()?,
        is_complete: page.is_complete(),
    })
}

fn decode_card(card: fb::TimelineEventCard<'_>) -> Result<EventCardWire, String> {
    let event_id = array_32(
        card.event_id().ok_or("card missing event_id")?.bytes(),
        "event_id",
    )?;
    let pubkey = array_32(
        card.pubkey().ok_or("card missing pubkey")?.bytes(),
        "pubkey",
    )?;

    let counts = card.relation_counts();
    let attribution = card.repost_attribution();

    Ok(EventCardWire {
        event_id,
        pubkey,
        created_at: card.created_at(),
        kind: card.kind(),
        content_json: card.content_json().unwrap_or_default().to_string(),
        reply_count: counts.map(|c| c.replies()).unwrap_or_default(),
        reaction_count: counts.map(|c| c.reactions()).unwrap_or_default(),
        repost_count: counts.map(|c| c.reposts()).unwrap_or_default(),
        zap_msats: counts.map(|c| c.zap_msats()).unwrap_or_default(),
        root_event_id: card
            .root_event_id()
            .map(|v| array_32(v.bytes(), "root_event_id"))
            .transpose()?,
        repost_original_pubkey: attribution
            .and_then(|a| a.original_author_pubkey())
            .map(|v| array_32(v.bytes(), "original_author_pubkey"))
            .transpose()?,
        repost_original_event_id: attribution
            .and_then(|a| a.original_event_id())
            .map(|v| array_32(v.bytes(), "original_event_id"))
            .transpose()?,
    })
}

/// Convert a wire byte slice into a fixed 32-byte identity array, rejecting
/// any slice whose length is not exactly 32 (raw pubkeys / event ids).
fn array_32(bytes: &[u8], field: &str) -> Result<[u8; 32], String> {
    bytes
        .try_into()
        .map_err(|_| format!("{field} must be 32 bytes, got {}", bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_wire() -> HomeFeedWire {
        HomeFeedWire {
            pages: vec![FeedPageWire {
                cards: vec![EventCardWire {
                    event_id: [1u8; 32],
                    pubkey: [2u8; 32],
                    created_at: 1_700_000_000,
                    kind: 1,
                    content_json: "hello world".into(),
                    reply_count: 3,
                    reaction_count: 5,
                    repost_count: 1,
                    zap_msats: 21_000,
                    root_event_id: None,
                    repost_original_pubkey: None,
                    repost_original_event_id: None,
                }],
                cursor_oldest_created_at: 1_700_000_000,
                cursor_oldest_event_id: Some([1u8; 32]),
                is_complete: false,
            }],
            window_limit: 50,
            window_offset: 0,
            total_items: 1,
        }
    }

    #[test]
    fn home_feed_wire_round_trips() {
        let wire = sample_wire();
        let encoded = encode_home_feed(&wire);
        assert!(!encoded.is_empty(), "encoded must not be empty");
        let decoded = decode_home_feed(&encoded).expect("must decode");
        assert_eq!(decoded.pages.len(), 1);
        assert_eq!(decoded.pages[0].cards.len(), 1);
        assert_eq!(decoded.pages[0].cards[0].event_id, [1u8; 32]);
        assert_eq!(decoded.pages[0].cards[0].content_json, "hello world");
    }

    #[test]
    fn full_snapshot_round_trips_byte_for_byte() {
        let wire = sample_wire();
        let decoded = decode_home_feed(&encode_home_feed(&wire)).expect("decode");
        assert_eq!(decoded, wire, "typed wire must round-trip losslessly");
    }

    #[test]
    fn encoded_buffer_carries_file_identifier() {
        let encoded = encode_home_feed(&sample_wire());
        assert!(
            fb::home_feed_snapshot_buffer_has_identifier(&encoded),
            "buffer must carry the NFHF identifier"
        );
        // The identifier lives at bytes 4..8 of a finished FlatBuffer.
        assert_eq!(&encoded[4..8], FILE_IDENTIFIER);
    }

    #[test]
    fn decode_rejects_buffer_without_identifier() {
        let err = decode_home_feed(&[0u8; 16]).expect_err("must reject");
        assert!(err.contains("NFHF"), "error names the missing id: {err}");
    }

    #[test]
    fn repost_card_round_trips_attribution() {
        let wire = HomeFeedWire {
            pages: vec![FeedPageWire {
                cards: vec![EventCardWire {
                    event_id: [9u8; 32],
                    pubkey: [8u8; 32],
                    created_at: 42,
                    kind: 6,
                    content_json: String::new(),
                    reply_count: 0,
                    reaction_count: 0,
                    repost_count: 0,
                    zap_msats: 0,
                    root_event_id: Some([7u8; 32]),
                    repost_original_pubkey: Some([6u8; 32]),
                    repost_original_event_id: Some([5u8; 32]),
                }],
                cursor_oldest_created_at: 42,
                cursor_oldest_event_id: None,
                is_complete: true,
            }],
            window_limit: 80,
            window_offset: 10,
            total_items: 1,
        };
        let decoded = decode_home_feed(&encode_home_feed(&wire)).expect("decode");
        let card = &decoded.pages[0].cards[0];
        assert_eq!(card.root_event_id, Some([7u8; 32]));
        assert_eq!(card.repost_original_pubkey, Some([6u8; 32]));
        assert_eq!(card.repost_original_event_id, Some([5u8; 32]));
        assert!(decoded.pages[0].is_complete);
        assert_eq!(decoded.window_offset, 10);
    }

    #[test]
    fn schema_constants_are_stable() {
        assert_eq!(SCHEMA_ID, "nmp.feed.home");
        assert_eq!(FILE_IDENTIFIER, b"NFHF");
        assert_eq!(SCHEMA_VERSION, 1);
    }

    #[test]
    fn empty_snapshot_round_trips() {
        let decoded =
            decode_home_feed(&encode_home_feed(&HomeFeedWire::default())).expect("decode");
        assert_eq!(decoded, HomeFeedWire::default());
    }
}
