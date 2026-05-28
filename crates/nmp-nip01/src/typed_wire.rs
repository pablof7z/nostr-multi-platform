//! Typed FlatBuffers wire encoding for `nmp_nip01::ModularTimelineSnapshot`.
//!
//! This is the nmp-nip01-owned typed projection of the *complete* assembled
//! home-feed snapshot: one `nmp.nip01.timeline.ModularTimelineSnapshot` buffer
//! (file identifier `NFTS`) carrying every block, every render-ready card, the
//! page cursor, and the window metrics. Hosts decode each field by offset
//! instead of walking a generic `Value` tree.
//!
//! Relates to ADR-0035 (typed FlatBuffers runtime projections) and ADR-0032
//! (raw-data projection doctrine). ADR-0035's authorized pilot is the
//! `nmp.feed.home` projection (`crates/nmp-feed/schema/feed_home.fbs`); this
//! schema co-exists with it as the nmp-nip01 full-snapshot view, pending
//! ADR-0035 reconciliation of the two card-shape owners.
//!
//! ## Parity, not simplification
//!
//! Every field of the serde [`ModularTimelineSnapshot`] survives the round
//! trip:
//! - [`TimelineBlock`] (`Standalone` / `Module`, with optional [`ThreadPointer`]).
//! - [`TimelineEventCard`] including its [`AuthorDisplay`], the
//!   [`NoteRelationCounts`] `Known` / `Loading` states (with their nested
//!   [`RelationCountInterest`]), the flat display mirrors, the content preview,
//!   and the optional [`RepostAttribution`].
//! - The optional `page` ([`TimelineWindowPage`]) and `metrics`
//!   ([`TimelineWindowMetrics`]).
//!
//! ## Opaque sub-payloads
//!
//! `content_tree` ([`ContentTreeWire`]) is embedded as the typed `nmp-content`
//! FlatBuffers buffer (`schema_id "nmp.content.tree"`, file identifier `NFCT`)
//! via [`nmp_content::wire::typed_fb::encode_content_tree`]. `content_render`
//! ([`crate::timeline_projection::ContentRenderData`], owned by this crate) is
//! embedded as opaque `serde_json` bytes for now. Both ride as opaque byte
//! fields, keeping this schema stable against their churn.
//!
//! TODO: swap the `content_render` serde_json payload for a typed encoder once
//! one exists.
//!
//! ## Regenerating the bindings
//!
//! The checked-in bindings in `wire/generated/timeline_snapshot_generated.rs`
//! are produced by `flatc` from `schema/timeline_snapshot.fbs`. Regenerate
//! only with the workspace FlatBuffers pin (`25.12.19`), enforced by
//! `ci/check-flatbuffers-version-pins.sh`:
//!
//! ```sh
//! flatc --rust -o crates/nmp-nip01/src/wire/generated \
//!       crates/nmp-nip01/schema/timeline_snapshot.fbs
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
#[path = "wire/generated/timeline_snapshot_generated.rs"]
mod timeline_snapshot_generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use nmp_content::ContentTreeWire;
use nmp_threading::{ThreadPointer, TimelineBlock};

use timeline_snapshot_generated::nmp::nip_01 as fb;

use crate::note_relations::{NoteRelationCounts, RelationCount, RelationCountInterest};
use crate::profile_display::AuthorDisplay;
use crate::timeline_projection::{
    ContentRenderData, ModularTimelineSnapshot, RepostAttribution, TimelineEventCard,
    TimelineWindowCursor, TimelineWindowMetrics, TimelineWindowPage,
};

/// Stable projection identifier this wire shape projects into.
pub const SCHEMA_ID: &str = "nmp.nip01.timeline";

/// FlatBuffers file identifier for a `ModularTimelineSnapshot` root buffer.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NFTS";

/// Schema version of the typed timeline-snapshot payload. Bump on any breaking
/// field change. Mirrors `ModularTimelineSnapshot.schema_version` in the `.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// ===========================================================================
// Encode
// ===========================================================================

/// Encode a [`ModularTimelineSnapshot`] as one typed FlatBuffers
/// `ModularTimelineSnapshot` buffer with the `NFTS` file identifier.
#[must_use]
pub fn encode_modular_timeline_snapshot(snapshot: &ModularTimelineSnapshot) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();

    let blocks: Vec<WIPOffset<fb::TimelineBlockEntry<'_>>> = snapshot
        .blocks
        .iter()
        .map(|block| encode_block(&mut builder, block))
        .collect();
    let blocks = builder.create_vector(&blocks);

    let cards: Vec<WIPOffset<fb::TimelineEventCard<'_>>> = snapshot
        .cards
        .iter()
        .map(|card| encode_card(&mut builder, card))
        .collect();
    let cards = builder.create_vector(&cards);

    let page = snapshot
        .page
        .as_ref()
        .map(|page| encode_page(&mut builder, page));
    let metrics = snapshot
        .metrics
        .as_ref()
        .map(|metrics| encode_metrics(&mut builder, metrics));

    let root = fb::ModularTimelineSnapshot::create(
        &mut builder,
        &fb::ModularTimelineSnapshotArgs {
            schema_version: SCHEMA_VERSION,
            blocks: Some(blocks),
            cards: Some(cards),
            page,
            metrics,
        },
    );
    fb::finish_modular_timeline_snapshot_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

fn encode_thread_pointer<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    pointer: &ThreadPointer,
) -> WIPOffset<fb::ThreadPointer<'bldr>> {
    match pointer {
        ThreadPointer::Event { id, relay, kind } => {
            let id = builder.create_string(id);
            let relay = relay.as_ref().map(|r| builder.create_string(r));
            fb::ThreadPointer::create(
                builder,
                &fb::ThreadPointerArgs {
                    kind: fb::ThreadPointerKind::Event,
                    id: Some(id),
                    coord: None,
                    uri: None,
                    relay,
                    has_kind_num: kind.is_some(),
                    kind_num: kind.unwrap_or_default(),
                },
            )
        }
        ThreadPointer::Address { coord, relay, kind } => {
            let coord = builder.create_string(coord);
            let relay = relay.as_ref().map(|r| builder.create_string(r));
            fb::ThreadPointer::create(
                builder,
                &fb::ThreadPointerArgs {
                    kind: fb::ThreadPointerKind::Address,
                    id: None,
                    coord: Some(coord),
                    uri: None,
                    relay,
                    has_kind_num: kind.is_some(),
                    kind_num: kind.unwrap_or_default(),
                },
            )
        }
        ThreadPointer::External { uri } => {
            let uri = builder.create_string(uri);
            fb::ThreadPointer::create(
                builder,
                &fb::ThreadPointerArgs {
                    kind: fb::ThreadPointerKind::External,
                    id: None,
                    coord: None,
                    uri: Some(uri),
                    relay: None,
                    has_kind_num: false,
                    kind_num: 0,
                },
            )
        }
    }
}

fn encode_block<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    block: &TimelineBlock,
) -> WIPOffset<fb::TimelineBlockEntry<'bldr>> {
    match block {
        TimelineBlock::Standalone { id, root } => {
            let standalone_id = builder.create_string(id);
            let standalone_root = root.as_ref().map(|r| encode_thread_pointer(builder, r));
            fb::TimelineBlockEntry::create(
                builder,
                &fb::TimelineBlockEntryArgs {
                    kind: fb::TimelineBlockKind::Standalone,
                    standalone_id: Some(standalone_id),
                    standalone_root,
                    module_event_ids: None,
                    module_has_gap: false,
                    module_root: None,
                },
            )
        }
        TimelineBlock::Module {
            events,
            has_gap,
            root,
        } => {
            let module_root = root.as_ref().map(|r| encode_thread_pointer(builder, r));
            let entries: Vec<WIPOffset<fb::BlockEventId<'_>>> = events
                .iter()
                .map(|event_id| {
                    let id = builder.create_string(event_id);
                    fb::BlockEventId::create(builder, &fb::BlockEventIdArgs { id: Some(id) })
                })
                .collect();
            let module_event_ids = builder.create_vector(&entries);
            fb::TimelineBlockEntry::create(
                builder,
                &fb::TimelineBlockEntryArgs {
                    kind: fb::TimelineBlockKind::Module,
                    standalone_id: None,
                    standalone_root: None,
                    module_event_ids: Some(module_event_ids),
                    module_has_gap: *has_gap,
                    module_root,
                },
            )
        }
    }
}

fn encode_author_display<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    display: &AuthorDisplay,
) -> WIPOffset<fb::AuthorDisplay<'bldr>> {
    let name = display.name.as_ref().map(|s| builder.create_string(s));
    let npub = display.npub.as_ref().map(|s| builder.create_string(s));
    let picture_url = display
        .picture_url
        .as_ref()
        .map(|s| builder.create_string(s));
    fb::AuthorDisplay::create(
        builder,
        &fb::AuthorDisplayArgs {
            has_name: display.name.is_some(),
            name,
            has_npub: display.npub.is_some(),
            npub,
            has_picture_url: display.picture_url.is_some(),
            picture_url,
        },
    )
}

fn encode_relation_count<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    count: &RelationCount,
) -> WIPOffset<fb::RelationCount<'bldr>> {
    match count {
        RelationCount::Known { count } => fb::RelationCount::create(
            builder,
            &fb::RelationCountArgs {
                state: fb::RelationCountState::Known,
                count: *count,
                interest: None,
            },
        ),
        RelationCount::Loading { interest } => {
            let namespace = builder.create_string(&interest.namespace);
            let target_event_id = builder.create_string(&interest.target_event_id);
            let tag = builder.create_string(&interest.tag);
            let interest = fb::RelationCountInterest::create(
                builder,
                &fb::RelationCountInterestArgs {
                    namespace: Some(namespace),
                    target_event_id: Some(target_event_id),
                    tag: Some(tag),
                },
            );
            fb::RelationCount::create(
                builder,
                &fb::RelationCountArgs {
                    state: fb::RelationCountState::Loading,
                    count: 0,
                    interest: Some(interest),
                },
            )
        }
    }
}

fn encode_relation_counts<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    counts: &NoteRelationCounts,
) -> WIPOffset<fb::NoteRelationCounts<'bldr>> {
    let replies = encode_relation_count(builder, &counts.replies);
    let reactions = encode_relation_count(builder, &counts.reactions);
    let reposts = encode_relation_count(builder, &counts.reposts);
    let zaps = encode_relation_count(builder, &counts.zaps);
    fb::NoteRelationCounts::create(
        builder,
        &fb::NoteRelationCountsArgs {
            replies: Some(replies),
            reactions: Some(reactions),
            reposts: Some(reposts),
            zaps: Some(zaps),
        },
    )
}

fn encode_repost_attribution<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    attribution: &RepostAttribution,
) -> WIPOffset<fb::RepostAttribution<'bldr>> {
    let author_display = encode_author_display(builder, &attribution.author_display);
    let author_pubkey = builder.create_string(&attribution.author_pubkey);
    let author_display_name = attribution
        .author_display_name
        .as_ref()
        .map(|s| builder.create_string(s));
    let author_picture_url = attribution
        .author_picture_url
        .as_ref()
        .map(|s| builder.create_string(s));
    fb::RepostAttribution::create(
        builder,
        &fb::RepostAttributionArgs {
            author_pubkey: Some(author_pubkey),
            author_display: Some(author_display),
            has_author_display_name: attribution.author_display_name.is_some(),
            author_display_name,
            has_author_picture_url: attribution.author_picture_url.is_some(),
            author_picture_url,
            note_created_at: attribution.note_created_at,
        },
    )
}

fn encode_card<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    card: &TimelineEventCard,
) -> WIPOffset<fb::TimelineEventCard<'bldr>> {
    // Child offsets must be created before the parent table.
    let author_display = encode_author_display(builder, &card.author_display);
    let relation_counts = encode_relation_counts(builder, &card.relation_counts);
    let reposted_by = card
        .reposted_by
        .as_ref()
        .map(|attribution| encode_repost_attribution(builder, attribution));

    let id = builder.create_string(&card.id);
    let author_pubkey = builder.create_string(&card.author_pubkey);
    let content = builder.create_string(&card.content);
    let content_preview = builder.create_string(&card.content_preview);
    let author_display_name = card
        .author_display_name
        .as_ref()
        .map(|s| builder.create_string(s));
    let author_picture_url = card
        .author_picture_url
        .as_ref()
        .map(|s| builder.create_string(s));

    let content_tree_bytes = encode_content_tree_bytes(&card.content_tree);
    let content_tree_bytes = builder.create_vector(&content_tree_bytes);
    let content_render_bytes = encode_content_render_bytes(&card.content_render);
    let content_render_bytes = builder.create_vector(&content_render_bytes);

    fb::TimelineEventCard::create(
        builder,
        &fb::TimelineEventCardArgs {
            id: Some(id),
            author_pubkey: Some(author_pubkey),
            author_display: Some(author_display),
            kind: card.kind,
            created_at: card.created_at,
            content: Some(content),
            content_tree_bytes: Some(content_tree_bytes),
            content_render_bytes: Some(content_render_bytes),
            relation_counts: Some(relation_counts),
            has_author_display_name: card.author_display_name.is_some(),
            author_display_name,
            has_author_picture_url: card.author_picture_url.is_some(),
            author_picture_url,
            content_preview: Some(content_preview),
            reposted_by,
        },
    )
}

fn encode_page<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    page: &TimelineWindowPage,
) -> WIPOffset<fb::FeedPage<'bldr>> {
    let next_cursor = page
        .next_cursor
        .as_ref()
        .map(|cursor| encode_cursor(builder, cursor));
    fb::FeedPage::create(
        builder,
        &fb::FeedPageArgs {
            limit: page.limit as u64,
            next_cursor,
            has_more: page.has_more,
            total_blocks: page.total_blocks as u64,
        },
    )
}

fn encode_cursor<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    cursor: &TimelineWindowCursor,
) -> WIPOffset<fb::FeedCursor<'bldr>> {
    let id = builder.create_string(&cursor.id);
    fb::FeedCursor::create(
        builder,
        &fb::FeedCursorArgs {
            created_at: cursor.created_at,
            id: Some(id),
        },
    )
}

fn encode_metrics<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    metrics: &TimelineWindowMetrics,
) -> WIPOffset<fb::FeedWindowMetrics<'bldr>> {
    fb::FeedWindowMetrics::create(
        builder,
        &fb::FeedWindowMetricsArgs {
            make_window_us: metrics.make_window_us,
        },
    )
}

/// Encode a [`ContentTreeWire`] as opaque bytes for embedding in a card.
///
/// Uses the typed `nmp-content` FlatBuffers encoder (`NFCT` buffer) so the
/// per-card content tree rides the typed path end-to-end.
fn encode_content_tree_bytes(tree: &ContentTreeWire) -> Vec<u8> {
    nmp_content::wire::typed_fb::encode_content_tree(tree)
}

/// Encode a [`ContentRenderData`] as opaque bytes for embedding in a card.
///
/// Currently serde_json — `ContentRenderData` is owned by this crate and has
/// no typed FlatBuffers encoder yet. TODO: swap to a typed encoder once
/// `nmp_content` (or this crate) exposes `encode_content_render_data`.
fn encode_content_render_bytes(render: &ContentRenderData) -> Vec<u8> {
    serde_json::to_vec(render).unwrap_or_default()
}

// ===========================================================================
// Decode
// ===========================================================================

/// Decode a typed FlatBuffers `ModularTimelineSnapshot` buffer back into the
/// owned [`ModularTimelineSnapshot`]. Returns a human-readable error string on
/// any malformed-buffer or missing-required-field condition.
pub fn decode_modular_timeline_snapshot(
    bytes: &[u8],
) -> Result<ModularTimelineSnapshot, String> {
    if !fb::modular_timeline_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NFTS file identifier".to_string());
    }
    let snapshot =
        fb::root_as_modular_timeline_snapshot(bytes).map_err(|err| format!("{err:?}"))?;

    let mut blocks = Vec::new();
    if let Some(fb_blocks) = snapshot.blocks() {
        blocks.reserve(fb_blocks.len());
        for index in 0..fb_blocks.len() {
            blocks.push(decode_block(fb_blocks.get(index))?);
        }
    }

    let mut cards = Vec::new();
    if let Some(fb_cards) = snapshot.cards() {
        cards.reserve(fb_cards.len());
        for index in 0..fb_cards.len() {
            cards.push(decode_card(fb_cards.get(index))?);
        }
    }

    let page = snapshot.page().map(decode_page).transpose()?;
    let metrics = snapshot.metrics().map(decode_metrics);

    Ok(ModularTimelineSnapshot {
        blocks,
        cards,
        page,
        metrics,
    })
}

fn decode_thread_pointer(pointer: fb::ThreadPointer<'_>) -> Result<ThreadPointer, String> {
    let kind = optional_kind_num(pointer.has_kind_num(), pointer.kind_num());
    let relay = pointer.relay().map(str::to_string);
    match pointer.kind() {
        fb::ThreadPointerKind::Event => Ok(ThreadPointer::Event {
            id: pointer
                .id()
                .ok_or("Event ThreadPointer missing id")?
                .to_string(),
            relay,
            kind,
        }),
        fb::ThreadPointerKind::Address => Ok(ThreadPointer::Address {
            coord: pointer
                .coord()
                .ok_or("Address ThreadPointer missing coord")?
                .to_string(),
            relay,
            kind,
        }),
        fb::ThreadPointerKind::External => Ok(ThreadPointer::External {
            uri: pointer
                .uri()
                .ok_or("External ThreadPointer missing uri")?
                .to_string(),
        }),
        other => Err(format!("unknown ThreadPointerKind: {other:?}")),
    }
}

fn decode_block(block: fb::TimelineBlockEntry<'_>) -> Result<TimelineBlock, String> {
    match block.kind() {
        fb::TimelineBlockKind::Standalone => {
            let id = block
                .standalone_id()
                .ok_or("Standalone block missing standalone_id")?
                .to_string();
            let root = block.standalone_root().map(decode_thread_pointer).transpose()?;
            Ok(TimelineBlock::Standalone { id, root })
        }
        fb::TimelineBlockKind::Module => {
            let mut events = Vec::new();
            if let Some(ids) = block.module_event_ids() {
                events.reserve(ids.len());
                for index in 0..ids.len() {
                    events.push(
                        ids.get(index)
                            .id()
                            .ok_or("Module block event id missing")?
                            .to_string(),
                    );
                }
            }
            let root = block.module_root().map(decode_thread_pointer).transpose()?;
            Ok(TimelineBlock::Module {
                events,
                has_gap: block.module_has_gap(),
                root,
            })
        }
        other => Err(format!("unknown TimelineBlockKind: {other:?}")),
    }
}

fn decode_author_display(display: fb::AuthorDisplay<'_>) -> AuthorDisplay {
    AuthorDisplay {
        name: optional_string(display.has_name(), display.name()),
        npub: optional_string(display.has_npub(), display.npub()),
        picture_url: optional_string(display.has_picture_url(), display.picture_url()),
    }
}

fn decode_relation_count(count: fb::RelationCount<'_>) -> Result<RelationCount, String> {
    match count.state() {
        fb::RelationCountState::Known => Ok(RelationCount::Known {
            count: count.count(),
        }),
        fb::RelationCountState::Loading => {
            let interest = count
                .interest()
                .ok_or("Loading RelationCount missing interest")?;
            Ok(RelationCount::Loading {
                interest: RelationCountInterest {
                    namespace: interest.namespace().unwrap_or_default().to_string(),
                    target_event_id: interest.target_event_id().unwrap_or_default().to_string(),
                    tag: interest.tag().unwrap_or_default().to_string(),
                },
            })
        }
        other => Err(format!("unknown RelationCountState: {other:?}")),
    }
}

fn decode_relation_counts(
    counts: fb::NoteRelationCounts<'_>,
) -> Result<NoteRelationCounts, String> {
    Ok(NoteRelationCounts {
        replies: decode_relation_count(counts.replies().ok_or("counts missing replies")?)?,
        reactions: decode_relation_count(counts.reactions().ok_or("counts missing reactions")?)?,
        reposts: decode_relation_count(counts.reposts().ok_or("counts missing reposts")?)?,
        zaps: decode_relation_count(counts.zaps().ok_or("counts missing zaps")?)?,
    })
}

fn decode_repost_attribution(
    attribution: fb::RepostAttribution<'_>,
) -> Result<RepostAttribution, String> {
    let author_display = decode_author_display(
        attribution
            .author_display()
            .ok_or("RepostAttribution missing author_display")?,
    );
    Ok(RepostAttribution {
        author_pubkey: attribution
            .author_pubkey()
            .ok_or("RepostAttribution missing author_pubkey")?
            .to_string(),
        author_display,
        author_display_name: optional_string(
            attribution.has_author_display_name(),
            attribution.author_display_name(),
        ),
        author_picture_url: optional_string(
            attribution.has_author_picture_url(),
            attribution.author_picture_url(),
        ),
        note_created_at: attribution.note_created_at(),
    })
}

fn decode_card(card: fb::TimelineEventCard<'_>) -> Result<TimelineEventCard, String> {
    let author_display =
        decode_author_display(card.author_display().ok_or("card missing author_display")?);
    let relation_counts =
        decode_relation_counts(card.relation_counts().ok_or("card missing relation_counts")?)?;
    let reposted_by = card.reposted_by().map(decode_repost_attribution).transpose()?;

    let content_tree = decode_content_tree_bytes(card.content_tree_bytes())?;
    let content_render = decode_content_render_bytes(card.content_render_bytes())?;

    Ok(TimelineEventCard {
        id: card.id().ok_or("card missing id")?.to_string(),
        author_pubkey: card
            .author_pubkey()
            .ok_or("card missing author_pubkey")?
            .to_string(),
        author_display,
        kind: card.kind(),
        created_at: card.created_at(),
        content: card.content().unwrap_or_default().to_string(),
        content_tree,
        content_render,
        relation_counts,
        author_display_name: optional_string(
            card.has_author_display_name(),
            card.author_display_name(),
        ),
        author_picture_url: optional_string(
            card.has_author_picture_url(),
            card.author_picture_url(),
        ),
        content_preview: card.content_preview().unwrap_or_default().to_string(),
        reposted_by,
    })
}

fn decode_page(page: fb::FeedPage<'_>) -> Result<TimelineWindowPage, String> {
    let next_cursor = page.next_cursor().map(decode_cursor).transpose()?;
    Ok(TimelineWindowPage {
        limit: page.limit() as usize,
        next_cursor,
        has_more: page.has_more(),
        total_blocks: page.total_blocks() as usize,
    })
}

fn decode_cursor(cursor: fb::FeedCursor<'_>) -> Result<TimelineWindowCursor, String> {
    Ok(TimelineWindowCursor {
        created_at: cursor.created_at(),
        id: cursor.id().ok_or("cursor missing id")?.to_string(),
    })
}

fn decode_metrics(metrics: fb::FeedWindowMetrics<'_>) -> TimelineWindowMetrics {
    TimelineWindowMetrics {
        make_window_us: metrics.make_window_us(),
    }
}

/// Decode opaque content-tree bytes back into a [`ContentTreeWire`] via the
/// typed `nmp-content` FlatBuffers decoder. Absent or empty bytes decode to the
/// default (empty) tree, matching the serde shape.
fn decode_content_tree_bytes(
    bytes: Option<flatbuffers::Vector<'_, u8>>,
) -> Result<ContentTreeWire, String> {
    match bytes {
        Some(v) if !v.bytes().is_empty() => {
            nmp_content::wire::typed_fb::decode_content_tree(v.bytes())
                .map_err(|err| format!("content_tree: {err}"))
        }
        _ => Ok(ContentTreeWire::default()),
    }
}

/// Decode opaque content-render bytes back into a [`ContentRenderData`]. Absent
/// or empty bytes decode to the default (empty) render data.
fn decode_content_render_bytes(
    bytes: Option<flatbuffers::Vector<'_, u8>>,
) -> Result<ContentRenderData, String> {
    match bytes {
        Some(v) if !v.bytes().is_empty() => {
            serde_json::from_slice(v.bytes()).map_err(|err| format!("content_render_json: {err}"))
        }
        _ => Ok(ContentRenderData::default()),
    }
}

/// Reconstruct an `Option<String>` from a `has_*` flag + the wire string,
/// distinguishing absent (`None`) from present-empty (`Some("")`).
fn optional_string(present: bool, value: Option<&str>) -> Option<String> {
    if present {
        Some(value.unwrap_or_default().to_string())
    } else {
        None
    }
}

/// Reconstruct an `Option<u32>` kind discriminator from the `has_kind_num`
/// flag + the wire value, distinguishing absent (`None`) from `Some(0)`.
fn optional_kind_num(present: bool, value: u32) -> Option<u32> {
    present.then_some(value)
}

#[cfg(test)]
#[path = "typed_wire/tests.rs"]
mod tests;
