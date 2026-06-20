use nmp_content::ContentTreeWire;
use nmp_feed::FeedWindowWire;
use nmp_threading::{ThreadPointer, TimelineBlock};

use super::fb;
use crate::note_relations::{NoteRelationCounts, RelationCount, RelationCountInterest};
use crate::timeline_projection::{ModularTimelineSnapshot, RepostAttribution, TimelineEventCard};

// ===========================================================================
// Decode
// ===========================================================================

/// Decode a typed FlatBuffers `ModularTimelineSnapshot` buffer back into the
/// owned [`ModularTimelineSnapshot`]. Returns a human-readable error string on
/// any malformed-buffer or missing-required-field condition.
pub fn decode_modular_timeline_snapshot(bytes: &[u8]) -> Result<ModularTimelineSnapshot, String> {
    if bytes.len() < 8 || !fb::modular_timeline_snapshot_buffer_has_identifier(bytes) {
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

    let feed_window = decode_feed_window_bytes(snapshot.feed_window_bytes())?;

    Ok(ModularTimelineSnapshot {
        blocks,
        cards,
        page: feed_window.page,
        metrics: feed_window.metrics,
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
            let root = block
                .standalone_root()
                .map(decode_thread_pointer)
                .transpose()?;
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
        // Appended field: pre-existing wire blobs omit it. Absence is a
        // known-zero comment count, not an error.
        comments: match counts.comments() {
            Some(comments) => decode_relation_count(comments)?,
            None => RelationCount::known(0),
        },
    })
}

fn decode_repost_attribution(
    attribution: fb::RepostAttribution<'_>,
) -> Result<RepostAttribution, String> {
    // GH #920: only the reposter's raw pubkey + `note_created_at` survive on
    // the struct. The display slots remain on the wire but are not read back.
    Ok(RepostAttribution {
        author_pubkey: attribution
            .author_pubkey()
            .ok_or("RepostAttribution missing author_pubkey")?
            .to_string(),
        note_created_at: attribution.note_created_at(),
    })
}

/// Decode one shared `nmp.nip01.TimelineEventCard` FlatBuffers table back into
/// the owned [`TimelineEventCard`].
///
/// `pub(crate)` so the OP-feed decoder (`crate::op_feed::typed_wire`) reuses the
/// identical per-card decoding — including the embedded typed NFCT content tree
/// and `content_render` — rather than re-deriving it (ADR-0038 Commitment 2).
pub(crate) fn decode_card(card: fb::TimelineEventCard<'_>) -> Result<TimelineEventCard, String> {
    let relation_counts = decode_relation_counts(
        card.relation_counts()
            .ok_or("card missing relation_counts")?,
    )?;
    let reposted_by = card
        .reposted_by()
        .map(decode_repost_attribution)
        .transpose()?;

    let content_tree = decode_content_tree_bytes(card.content_tree_bytes())?;
    let relay_provenance = card
        .relay_provenance()
        .map(|relays| relays.iter().map(str::to_string).collect())
        .unwrap_or_default();

    // GH #920: the card no longer carries `author_display`, the flat display
    // mirrors, `content_preview`, or `content_render`. The wire still ships
    // those slots (kept byte-identical pending the cross-language follow-up);
    // the decoder simply does not read them back into the slimmed struct.
    Ok(TimelineEventCard {
        id: card.id().ok_or("card missing id")?.to_string(),
        author_pubkey: card
            .author_pubkey()
            .ok_or("card missing author_pubkey")?
            .to_string(),
        kind: card.kind(),
        created_at: card.created_at(),
        content: card.content().unwrap_or_default().to_string(),
        content_tree,
        relation_counts,
        relay_provenance,
        reposted_by,
    })
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

/// Decode the embedded typed nmp-feed `FeedWindow` buffer. Absent or empty
/// bytes decode to the default empty window, matching the unpaged diagnostics
/// snapshot shape.
fn decode_feed_window_bytes(
    bytes: Option<flatbuffers::Vector<'_, u8>>,
) -> Result<FeedWindowWire, String> {
    match bytes {
        Some(v) if !v.bytes().is_empty() => {
            nmp_feed::decode_feed_window(v.bytes()).map_err(|err| format!("feed_window: {err}"))
        }
        _ => Ok(FeedWindowWire::default()),
    }
}

/// Reconstruct an `Option<u32>` kind discriminator from the `has_kind_num`
/// flag + the wire value, distinguishing absent (`None`) from `Some(0)`.
fn optional_kind_num(present: bool, value: u32) -> Option<u32> {
    present.then_some(value)
}
