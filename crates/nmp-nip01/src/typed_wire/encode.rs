use flatbuffers::{FlatBufferBuilder, WIPOffset};
use nmp_content::ContentTreeWire;
use nmp_feed::FeedWindowWire;
use nmp_threading::{ThreadPointer, TimelineBlock};

use super::{fb, SCHEMA_VERSION};
use crate::note_relations::{NoteRelationCounts, RelationCount};
use crate::profile_display::AuthorDisplay;
use crate::timeline_projection::{
    ContentEventRenderData, ContentProfileRenderData, ContentRenderData, ModularTimelineSnapshot,
    RepostAttribution, TimelineEventCard,
};

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

    let feed_window_bytes = encode_feed_window_bytes(snapshot)
        .as_ref()
        .map(|bytes| builder.create_vector(bytes));

    let root = fb::ModularTimelineSnapshot::create(
        &mut builder,
        &fb::ModularTimelineSnapshotArgs {
            schema_version: SCHEMA_VERSION,
            blocks: Some(blocks),
            cards: Some(cards),
            feed_window_bytes,
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
    let picture_url = display
        .picture_url
        .as_ref()
        .map(|s| builder.create_string(s));
    fb::AuthorDisplay::create(
        builder,
        &fb::AuthorDisplayArgs {
            has_name: display.name.is_some(),
            name,
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
    let comments = encode_relation_count(builder, &counts.comments);
    fb::NoteRelationCounts::create(
        builder,
        &fb::NoteRelationCountsArgs {
            replies: Some(replies),
            reactions: Some(reactions),
            reposts: Some(reposts),
            zaps: Some(zaps),
            comments: Some(comments),
        },
    )
}

fn encode_repost_attribution<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    attribution: &RepostAttribution,
) -> WIPOffset<fb::RepostAttribution<'bldr>> {
    // GH #920: `RepostAttribution` carries the reposter's raw pubkey +
    // `note_created_at` only — no denormalized display copy. The vestigial
    // wire slots (kept byte-identical pending the cross-language follow-up)
    // get the absent shape: a fallback display + `has_* = false`.
    let author_display = encode_author_display(
        builder,
        &AuthorDisplay::fallback(&attribution.author_pubkey),
    );
    let author_pubkey = builder.create_string(&attribution.author_pubkey);
    fb::RepostAttribution::create(
        builder,
        &fb::RepostAttributionArgs {
            author_pubkey: Some(author_pubkey),
            author_display: Some(author_display),
            has_author_display_name: false,
            author_display_name: None,
            has_author_picture_url: false,
            author_picture_url: None,
            note_created_at: attribution.note_created_at,
        },
    )
}

/// Encode one [`TimelineEventCard`] into the shared `nmp.nip01.TimelineEventCard`
/// FlatBuffers table.
///
/// `pub(crate)` so the OP-feed encoder (`crate::op_feed::typed_wire`) reuses the
/// identical per-card builder — including the embedded typed NFCT content tree
/// and `content_render` bytes — rather than re-deriving the card encoding
/// (ADR-0038 Commitment 2).
pub(crate) fn encode_card<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    card: &TimelineEventCard,
) -> WIPOffset<fb::TimelineEventCard<'bldr>> {
    // GH #920: `TimelineEventCard` no longer denormalizes any author display
    // copy (`author_display`, `author_display_name`, `author_picture_url`),
    // the render preview (`content_preview`), or the embedded-ref render facts
    // (`content_render`). The presentation layer joins against the snapshot's
    // `refs.profile` / `refs.event` keyed rows instead. The `.fbs` schema +
    // generated bindings stay byte-identical (the coordinated cross-language
    // wire-slot removal rides with the iOS/Android follow-up), so the encoder
    // fills the now-vestigial slots with the absent shape: a fallback
    // `AuthorDisplay`, empty preview,
    // `has_* = false`, and an empty `ContentRenderData`.
    let author_display =
        encode_author_display(builder, &AuthorDisplay::fallback(&card.author_pubkey));
    let relation_counts = encode_relation_counts(builder, &card.relation_counts);
    let reposted_by = card
        .reposted_by
        .as_ref()
        .map(|attribution| encode_repost_attribution(builder, attribution));

    let id = builder.create_string(&card.id);
    let author_pubkey = builder.create_string(&card.author_pubkey);
    let content = builder.create_string(&card.content);
    let content_preview = builder.create_string("");
    let relay_provenance_offsets: Vec<WIPOffset<&str>> = card
        .relay_provenance
        .iter()
        .map(|relay| builder.create_string(relay))
        .collect();
    let relay_provenance = builder.create_vector(&relay_provenance_offsets);

    let content_tree_bytes = encode_content_tree_bytes(&card.content_tree);
    let content_tree_bytes = builder.create_vector(&content_tree_bytes);
    let content_render = encode_content_render(builder, &ContentRenderData::default());

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
            content_render: Some(content_render),
            relation_counts: Some(relation_counts),
            has_author_display_name: false,
            author_display_name: None,
            has_author_picture_url: false,
            author_picture_url: None,
            content_preview: Some(content_preview),
            reposted_by,
            relay_provenance: Some(relay_provenance),
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

fn encode_content_render<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    render: &ContentRenderData,
) -> WIPOffset<fb::ContentRenderData<'bldr>> {
    let profiles: Vec<WIPOffset<fb::ContentProfileRenderEntry<'_>>> = render
        .profiles
        .iter()
        .map(|(key, profile)| encode_content_profile_render_entry(builder, key, profile))
        .collect();
    let profiles = builder.create_vector(&profiles);

    let events: Vec<WIPOffset<fb::ContentEventRenderEntry<'_>>> = render
        .events
        .iter()
        .map(|(key, event)| encode_content_event_render_entry(builder, key, event))
        .collect();
    let events = builder.create_vector(&events);

    fb::ContentRenderData::create(
        builder,
        &fb::ContentRenderDataArgs {
            profiles: Some(profiles),
            events: Some(events),
        },
    )
}

fn encode_content_profile_render_entry<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    key: &str,
    profile: &ContentProfileRenderData,
) -> WIPOffset<fb::ContentProfileRenderEntry<'bldr>> {
    let key = builder.create_string(key);
    let pubkey = builder.create_string(&profile.pubkey);
    let display = encode_author_display(builder, &profile.display);
    fb::ContentProfileRenderEntry::create(
        builder,
        &fb::ContentProfileRenderEntryArgs {
            key: Some(key),
            pubkey: Some(pubkey),
            display: Some(display),
        },
    )
}

fn encode_content_event_render_entry<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    key: &str,
    event: &ContentEventRenderData,
) -> WIPOffset<fb::ContentEventRenderEntry<'bldr>> {
    let key = builder.create_string(key);
    let id = builder.create_string(&event.id);
    let author_pubkey = builder.create_string(&event.author_pubkey);
    let author_display = encode_author_display(builder, &event.author_display);
    let content_preview = builder.create_string(&event.content_preview);
    let content_tree_bytes = encode_content_tree_bytes(&event.content_tree);
    let content_tree_bytes = builder.create_vector(&content_tree_bytes);
    fb::ContentEventRenderEntry::create(
        builder,
        &fb::ContentEventRenderEntryArgs {
            key: Some(key),
            id: Some(id),
            author_pubkey: Some(author_pubkey),
            author_display: Some(author_display),
            kind: event.kind,
            created_at: event.created_at,
            content_preview: Some(content_preview),
            content_tree_bytes: Some(content_tree_bytes),
        },
    )
}

fn encode_feed_window_bytes(snapshot: &ModularTimelineSnapshot) -> Option<Vec<u8>> {
    if snapshot.page.is_none() && snapshot.metrics.is_none() {
        return None;
    }
    Some(nmp_feed::encode_feed_window(&FeedWindowWire {
        page: snapshot.page.clone(),
        metrics: snapshot.metrics.clone(),
    }))
}
