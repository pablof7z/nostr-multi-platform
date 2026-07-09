//! Typed FlatBuffers wire for [`crate::RootFeedSnapshot<crate::FeedRow>`] — the
//! FROZEN feed-row shape (#3082 settled design).
//!
//! The checked-in bindings in `generated/feed_row_generated.rs` are produced by
//! `flatc` from `schema/feed_row.fbs`. Regenerate ONLY via
//! `ci/regenerate-flatbuffers.sh` (never raw `flatc` — it red-fails the
//! `ci/check-rust-flatc-drift.sh` gate).

#[allow(
    clippy::all,
    clippy::pedantic,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "generated/feed_row_generated.rs"]
mod feed_row_generated;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use feed_row_generated::nmp::feed as fb;

use crate::feed_row::{FeedRow, FeedRowContext};
use crate::typed_ref::{DeliveryMode, TypedRef, TypedRefTarget};
use crate::typed_wire::{decode_feed_window, encode_feed_window, FeedWindowWire};
use crate::{RootCard, RootFeedSnapshot};

/// Stable schema identifier for the frozen feed-row payload.
pub const FEED_ROW_SCHEMA_ID: &str = "nmp.feed.feed_row";

/// FlatBuffers file identifier for a `FeedRowSnapshot` root buffer.
pub const FEED_ROW_FILE_IDENTIFIER: &[u8; 4] = b"NFRS";

/// Schema version of the typed feed-row payload.
pub const FEED_ROW_SCHEMA_VERSION: u32 = 1;

/// The snapshot this module encodes: `RootFeedSnapshot<FeedRow>`.
pub type FeedRowSnapshot = RootFeedSnapshot<FeedRow>;

/// Encode a feed-row snapshot as one typed FlatBuffers `FeedRowSnapshot` root
/// buffer with the `NFRS` file identifier.
#[must_use]
pub fn encode_feed_row_snapshot(snapshot: &FeedRowSnapshot) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();
    let cards = if snapshot.cards.is_empty() {
        None
    } else {
        let offsets: Vec<_> = snapshot
            .cards
            .iter()
            .map(|card| encode_root_card(&mut builder, card))
            .collect();
        Some(builder.create_vector(&offsets))
    };
    let window = FeedWindowWire {
        page: snapshot.page.clone(),
        metrics: snapshot.metrics.clone(),
    };
    let feed_window_bytes = if snapshot.page.is_some() || snapshot.metrics.is_some() {
        Some(builder.create_vector(&encode_feed_window(&window)))
    } else {
        None
    };
    let root = fb::FeedRowSnapshot::create(
        &mut builder,
        &fb::FeedRowSnapshotArgs {
            schema_version: 1,
            cards,
            feed_window_bytes,
            has_page: snapshot.page.is_some(),
            has_metrics: snapshot.metrics.is_some(),
        },
    );
    fb::finish_feed_row_snapshot_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

/// Decode a typed FlatBuffers `FeedRowSnapshot` root buffer back into an owned
/// [`FeedRowSnapshot`].
pub fn decode_feed_row_snapshot(bytes: &[u8]) -> Result<FeedRowSnapshot, String> {
    if bytes.len() < 8 || !fb::feed_row_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NFRS file identifier".to_string());
    }
    let snapshot = fb::root_as_feed_row_snapshot(bytes).map_err(|err| format!("{err:?}"))?;

    let cards = snapshot
        .cards()
        .map(|cards| {
            cards
                .iter()
                .map(decode_root_card)
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();

    let window = snapshot
        .feed_window_bytes()
        .map(|bytes| decode_feed_window(bytes.bytes()))
        .transpose()?
        .unwrap_or_default();

    Ok(FeedRowSnapshot {
        cards,
        page: snapshot.has_page().then_some(()).and(window.page),
        metrics: snapshot.has_metrics().then_some(()).and(window.metrics),
    })
}

fn encode_root_card<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    card: &RootCard<FeedRow>,
) -> WIPOffset<fb::RootCard<'bldr>> {
    let row = encode_feed_row(builder, &card.card);
    fb::RootCard::create(builder, &fb::RootCardArgs { card: Some(row) })
}

fn decode_root_card(card: fb::RootCard<'_>) -> Result<RootCard<FeedRow>, String> {
    let row = card.card().ok_or("root card missing feed row")?;
    Ok(RootCard {
        card: decode_feed_row(row)?,
    })
}

fn encode_feed_row<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    row: &FeedRow,
) -> WIPOffset<fb::FeedRow<'bldr>> {
    let canonical_row_id = builder.create_string(&row.canonical_row_id);
    let source_id = builder.create_string(&row.source_id);
    let author_pubkey = builder.create_string(&row.author_pubkey);
    let content = builder.create_string(&row.content);

    let tags = if row.tags.is_empty() {
        None
    } else {
        let rows: Vec<_> = row
            .tags
            .iter()
            .map(|tag| {
                let values: Vec<_> = tag.iter().map(|v| builder.create_string(v)).collect();
                let values = builder.create_vector(&values);
                fb::TagRow::create(
                    builder,
                    &fb::TagRowArgs {
                        values: Some(values),
                    },
                )
            })
            .collect();
        Some(builder.create_vector(&rows))
    };

    let relay_provenance = if row.relay_provenance.is_empty() {
        None
    } else {
        let relays: Vec<_> = row
            .relay_provenance
            .iter()
            .map(|r| builder.create_string(r))
            .collect();
        Some(builder.create_vector(&relays))
    };

    let refs = if row.refs.is_empty() {
        None
    } else {
        let refs: Vec<_> = row
            .refs
            .iter()
            .map(|r| encode_typed_ref(builder, r))
            .collect();
        Some(builder.create_vector(&refs))
    };

    let context = if row.context.is_empty() {
        None
    } else {
        let entries: Vec<_> = row
            .context
            .iter()
            .map(|c| encode_context(builder, c))
            .collect();
        Some(builder.create_vector(&entries))
    };

    fb::FeedRow::create(
        builder,
        &fb::FeedRowArgs {
            canonical_row_id: Some(canonical_row_id),
            source_id: Some(source_id),
            author_pubkey: Some(author_pubkey),
            kind: row.kind,
            created_at: row.created_at,
            content: Some(content),
            tags,
            relay_provenance,
            refs,
            context,
        },
    )
}

fn decode_feed_row(row: fb::FeedRow<'_>) -> Result<FeedRow, String> {
    let tags = row
        .tags()
        .map(|tags| {
            tags.iter()
                .map(|tag_row| {
                    tag_row
                        .values()
                        .map(|values| values.iter().map(str::to_string).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();

    let relay_provenance = row
        .relay_provenance()
        .map(|relays| relays.iter().map(str::to_string).collect())
        .unwrap_or_default();

    let refs = row
        .refs()
        .map(|refs| {
            refs.iter()
                .map(decode_typed_ref)
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();

    let context = row
        .context()
        .map(|entries| {
            entries
                .iter()
                .map(decode_context)
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(FeedRow {
        canonical_row_id: row.canonical_row_id().unwrap_or_default().to_string(),
        source_id: row.source_id().unwrap_or_default().to_string(),
        author_pubkey: row.author_pubkey().unwrap_or_default().to_string(),
        kind: row.kind(),
        created_at: row.created_at(),
        content: row.content().unwrap_or_default().to_string(),
        tags,
        relay_provenance,
        refs,
        context,
    })
}

fn encode_typed_ref<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    typed_ref: &TypedRef,
) -> WIPOffset<fb::TypedRef<'bldr>> {
    let (target_type, target) = match &typed_ref.target {
        TypedRefTarget::EventId(id) => {
            let id = builder.create_string(id);
            let offset = fb::RefEventId::create(builder, &fb::RefEventIdArgs { id: Some(id) });
            (fb::RefTargetUnion::RefEventId, offset.as_union_value())
        }
        TypedRefTarget::Address { kind, pubkey, d } => {
            let pubkey = builder.create_string(pubkey);
            let d = builder.create_string(d);
            let offset = fb::RefAddress::create(
                builder,
                &fb::RefAddressArgs {
                    kind: *kind,
                    pubkey: Some(pubkey),
                    d: Some(d),
                },
            );
            (fb::RefTargetUnion::RefAddress, offset.as_union_value())
        }
    };
    let delivery_mode = match typed_ref.delivery_mode {
        DeliveryMode::RenderOnly => fb::DeliveryMode::RenderOnly,
        DeliveryMode::Delivered => fb::DeliveryMode::Delivered,
    };
    fb::TypedRef::create(
        builder,
        &fb::TypedRefArgs {
            target_type,
            target: Some(target),
            delivery_mode,
        },
    )
}

fn decode_typed_ref(typed_ref: fb::TypedRef<'_>) -> Result<TypedRef, String> {
    let target = match typed_ref.target_type() {
        fb::RefTargetUnion::RefEventId => {
            let table = typed_ref
                .target_as_ref_event_id()
                .ok_or("typed ref missing RefEventId table")?;
            TypedRefTarget::EventId(table.id().unwrap_or_default().to_string())
        }
        fb::RefTargetUnion::RefAddress => {
            let table = typed_ref
                .target_as_ref_address()
                .ok_or("typed ref missing RefAddress table")?;
            TypedRefTarget::Address {
                kind: table.kind(),
                pubkey: table.pubkey().unwrap_or_default().to_string(),
                d: table.d().unwrap_or_default().to_string(),
            }
        }
        other => return Err(format!("unknown ref target union variant: {other:?}")),
    };
    let delivery_mode = match typed_ref.delivery_mode() {
        fb::DeliveryMode::Delivered => DeliveryMode::Delivered,
        _ => DeliveryMode::RenderOnly,
    };
    Ok(TypedRef {
        target,
        delivery_mode,
    })
}

fn encode_context<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    context: &FeedRowContext,
) -> WIPOffset<fb::FeedRowContext<'bldr>> {
    let (value_type, value) = match context {
        FeedRowContext::Authored => {
            let offset = fb::Authored::create(builder, &fb::AuthoredArgs {});
            (fb::ProvenanceUnion::Authored, offset.as_union_value())
        }
        FeedRowContext::RepostedBy {
            author_pubkey,
            note_created_at,
        } => {
            let author_pubkey = builder.create_string(author_pubkey);
            let offset = fb::RepostedBy::create(
                builder,
                &fb::RepostedByArgs {
                    author_pubkey: Some(author_pubkey),
                    note_created_at: *note_created_at,
                },
            );
            (fb::ProvenanceUnion::RepostedBy, offset.as_union_value())
        }
        FeedRowContext::CommentedBy {
            author_pubkey,
            comment_event_id,
            comment_created_at,
        } => {
            let author_pubkey = builder.create_string(author_pubkey);
            let comment_event_id = builder.create_string(comment_event_id);
            let offset = fb::CommentedBy::create(
                builder,
                &fb::CommentedByArgs {
                    author_pubkey: Some(author_pubkey),
                    comment_event_id: Some(comment_event_id),
                    comment_created_at: *comment_created_at,
                },
            );
            (fb::ProvenanceUnion::CommentedBy, offset.as_union_value())
        }
        FeedRowContext::Group { relay, id } => {
            let relay = builder.create_string(relay);
            let id = builder.create_string(id);
            let offset = fb::GroupContext::create(
                builder,
                &fb::GroupContextArgs {
                    relay: Some(relay),
                    id: Some(id),
                },
            );
            (fb::ProvenanceUnion::GroupContext, offset.as_union_value())
        }
    };
    fb::FeedRowContext::create(
        builder,
        &fb::FeedRowContextArgs {
            value_type,
            value: Some(value),
        },
    )
}

fn decode_context(context: fb::FeedRowContext<'_>) -> Result<FeedRowContext, String> {
    match context.value_type() {
        fb::ProvenanceUnion::Authored => Ok(FeedRowContext::Authored),
        fb::ProvenanceUnion::RepostedBy => {
            let table = context
                .value_as_reposted_by()
                .ok_or("provenance missing RepostedBy table")?;
            Ok(FeedRowContext::RepostedBy {
                author_pubkey: table.author_pubkey().unwrap_or_default().to_string(),
                note_created_at: table.note_created_at(),
            })
        }
        fb::ProvenanceUnion::CommentedBy => {
            let table = context
                .value_as_commented_by()
                .ok_or("provenance missing CommentedBy table")?;
            Ok(FeedRowContext::CommentedBy {
                author_pubkey: table.author_pubkey().unwrap_or_default().to_string(),
                comment_event_id: table.comment_event_id().unwrap_or_default().to_string(),
                comment_created_at: table.comment_created_at(),
            })
        }
        fb::ProvenanceUnion::GroupContext => {
            let table = context
                .value_as_group_context()
                .ok_or("provenance missing GroupContext table")?;
            Ok(FeedRowContext::Group {
                relay: table.relay().unwrap_or_default().to_string(),
                id: table.id().unwrap_or_default().to_string(),
            })
        }
        other => Err(format!("unknown provenance union variant: {other:?}")),
    }
}

#[cfg(test)]
#[path = "feed_row_wire_tests.rs"]
mod tests;
