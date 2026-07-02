//! Typed FlatBuffers wire encoding for the note-feed OP-centric projection.
//!
//! The wire owner is `nmp-note-feed`; the low-level `nmp-nip01` crate does not
//! define or embed concrete feed row/card payloads.

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use nmp_feed::{FeedWindowWire, RootCard, RootFeedSnapshot, MAX_ATTRIBUTION_PER_ROOT};

use super::attribution::Nip10ReplyAttribution;
use crate::op_feed_generated::nmp::note_feed as fb;
use crate::{HostedGroupContext, NoteFeedItem, RepostAttribution};

pub type OpFeedSnapshot = RootFeedSnapshot<NoteFeedItem, Nip10ReplyAttribution>;

pub const OP_FEED_SCHEMA_ID: &str = "nmp.note_feed.opfeed";
pub const OP_FEED_FILE_IDENTIFIER: &[u8; 4] = b"NNFS";
pub const OP_FEED_SCHEMA_VERSION: u32 = 2;

#[must_use]
pub fn encode_op_feed_snapshot(snapshot: &OpFeedSnapshot) -> Vec<u8> {
    let mut builder = FlatBufferBuilder::new();

    let items: Vec<WIPOffset<fb::RootItem<'_>>> = snapshot
        .cards
        .iter()
        .map(|root| encode_root_item(&mut builder, root))
        .collect();
    let items = builder.create_vector(&items);

    let has_page = snapshot.page.is_some();
    let has_metrics = snapshot.metrics.is_some();
    let feed_window_bytes = encode_feed_window_bytes(snapshot)
        .as_ref()
        .map(|bytes| builder.create_vector(bytes));

    let root = fb::OpFeedSnapshot::create(
        &mut builder,
        &fb::OpFeedSnapshotArgs {
            schema_version: OP_FEED_SCHEMA_VERSION,
            items: Some(items),
            feed_window_bytes,
            has_page,
            has_metrics,
        },
    );
    fb::finish_op_feed_snapshot_buffer(&mut builder, root);
    builder.finished_data().to_vec()
}

fn encode_root_item<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    root: &RootCard<NoteFeedItem, Nip10ReplyAttribution>,
) -> WIPOffset<fb::RootItem<'bldr>> {
    let item = encode_note_feed_item(builder, &root.card);
    let attribution: Vec<WIPOffset<fb::ReplyAttribution<'_>>> = root
        .attribution
        .iter()
        .take(MAX_ATTRIBUTION_PER_ROOT)
        .map(|attr| encode_reply_attribution(builder, attr))
        .collect();
    let attribution = builder.create_vector(&attribution);

    fb::RootItem::create(
        builder,
        &fb::RootItemArgs {
            item: Some(item),
            attribution: Some(attribution),
        },
    )
}

fn encode_note_feed_item<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    item: &NoteFeedItem,
) -> WIPOffset<fb::NoteFeedItem<'bldr>> {
    let id = builder.create_string(&item.id);
    let author_pubkey = builder.create_string(&item.author_pubkey);
    let content = builder.create_string(&item.content);
    let content_tree_bytes = nmp_content::wire::typed_fb::encode_content_tree(&item.content_tree);
    let content_tree_bytes = builder.create_vector(&content_tree_bytes);
    let relay_provenance_offsets: Vec<WIPOffset<&str>> = item
        .relay_provenance
        .iter()
        .map(|relay| builder.create_string(relay))
        .collect();
    let relay_provenance = builder.create_vector(&relay_provenance_offsets);
    let reposted_by = item
        .reposted_by
        .as_ref()
        .map(|repost| encode_repost_attribution(builder, repost));
    let hosted_group = item
        .hosted_group
        .as_ref()
        .map(|context| encode_hosted_group_context(builder, context));

    fb::NoteFeedItem::create(
        builder,
        &fb::NoteFeedItemArgs {
            id: Some(id),
            author_pubkey: Some(author_pubkey),
            kind: item.kind,
            created_at: item.created_at,
            content: Some(content),
            content_tree_bytes: Some(content_tree_bytes),
            relay_provenance: Some(relay_provenance),
            reposted_by,
            hosted_group,
        },
    )
}

fn encode_repost_attribution<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    attr: &RepostAttribution,
) -> WIPOffset<fb::RepostAttribution<'bldr>> {
    let author_pubkey = builder.create_string(&attr.author_pubkey);
    fb::RepostAttribution::create(
        builder,
        &fb::RepostAttributionArgs {
            author_pubkey: Some(author_pubkey),
            note_created_at: attr.note_created_at,
        },
    )
}

fn encode_hosted_group_context<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    context: &HostedGroupContext,
) -> WIPOffset<fb::HostedGroupContext<'bldr>> {
    let host_relay_url = builder.create_string(&context.host_relay_url);
    let local_id = builder.create_string(&context.local_id);
    fb::HostedGroupContext::create(
        builder,
        &fb::HostedGroupContextArgs {
            host_relay_url: Some(host_relay_url),
            local_id: Some(local_id),
        },
    )
}

fn encode_reply_attribution<'bldr>(
    builder: &mut FlatBufferBuilder<'bldr>,
    attr: &Nip10ReplyAttribution,
) -> WIPOffset<fb::ReplyAttribution<'bldr>> {
    let author_pubkey = builder.create_string(&attr.author_pubkey);
    let reply_event_id = builder.create_string(&attr.reply_event_id);
    fb::ReplyAttribution::create(
        builder,
        &fb::ReplyAttributionArgs {
            author_pubkey: Some(author_pubkey),
            reply_event_id: Some(reply_event_id),
            reply_created_at: attr.reply_created_at,
        },
    )
}

fn encode_feed_window_bytes(snapshot: &OpFeedSnapshot) -> Option<Vec<u8>> {
    if snapshot.page.is_none() && snapshot.metrics.is_none() {
        return None;
    }
    Some(nmp_feed::encode_feed_window(&FeedWindowWire {
        page: snapshot.page.clone(),
        metrics: snapshot.metrics.clone(),
    }))
}

pub fn decode_op_feed_snapshot(bytes: &[u8]) -> Result<OpFeedSnapshot, String> {
    if bytes.len() < 8 || !fb::op_feed_snapshot_buffer_has_identifier(bytes) {
        return Err("missing NNFS file identifier".to_string());
    }
    let snapshot = fb::root_as_op_feed_snapshot(bytes).map_err(|err| format!("{err:?}"))?;

    let mut cards = Vec::new();
    if let Some(fb_items) = snapshot.items() {
        cards.reserve(fb_items.len());
        for index in 0..fb_items.len() {
            cards.push(decode_root_item(fb_items.get(index))?);
        }
    }

    let window = decode_feed_window_bytes(snapshot.feed_window_bytes())?;
    Ok(RootFeedSnapshot {
        cards,
        page: window.page,
        metrics: window.metrics,
    })
}

fn decode_root_item(
    root: fb::RootItem<'_>,
) -> Result<RootCard<NoteFeedItem, Nip10ReplyAttribution>, String> {
    let card = decode_note_feed_item(root.item().ok_or("RootItem missing item")?)?;
    let mut attribution = Vec::new();
    if let Some(attrs) = root.attribution() {
        attribution.reserve(attrs.len());
        for index in 0..attrs.len() {
            attribution.push(decode_reply_attribution(attrs.get(index))?);
        }
    }
    Ok(RootCard { card, attribution })
}

fn decode_note_feed_item(item: fb::NoteFeedItem<'_>) -> Result<NoteFeedItem, String> {
    let content_tree = match item.content_tree_bytes() {
        Some(bytes) if !bytes.bytes().is_empty() => {
            nmp_content::wire::typed_fb::decode_content_tree(bytes.bytes())
                .map_err(|err| format!("content_tree: {err}"))?
        }
        _ => Default::default(),
    };
    let relay_provenance = item
        .relay_provenance()
        .map(|relays| relays.iter().map(str::to_string).collect())
        .unwrap_or_default();
    Ok(NoteFeedItem {
        id: item.id().ok_or("NoteFeedItem missing id")?.to_string(),
        author_pubkey: item
            .author_pubkey()
            .ok_or("NoteFeedItem missing author_pubkey")?
            .to_string(),
        kind: item.kind(),
        created_at: item.created_at(),
        content: item.content().unwrap_or_default().to_string(),
        content_tree,
        relay_provenance,
        reposted_by: item
            .reposted_by()
            .map(decode_repost_attribution)
            .transpose()?,
        hosted_group: item
            .hosted_group()
            .map(decode_hosted_group_context)
            .transpose()?,
    })
}

fn decode_repost_attribution(attr: fb::RepostAttribution<'_>) -> Result<RepostAttribution, String> {
    Ok(RepostAttribution {
        author_pubkey: attr
            .author_pubkey()
            .ok_or("RepostAttribution missing author_pubkey")?
            .to_string(),
        note_created_at: attr.note_created_at(),
    })
}

fn decode_hosted_group_context(
    context: fb::HostedGroupContext<'_>,
) -> Result<HostedGroupContext, String> {
    Ok(HostedGroupContext {
        host_relay_url: context
            .host_relay_url()
            .ok_or("HostedGroupContext missing host_relay_url")?
            .to_string(),
        local_id: context
            .local_id()
            .ok_or("HostedGroupContext missing local_id")?
            .to_string(),
    })
}

fn decode_reply_attribution(
    attr: fb::ReplyAttribution<'_>,
) -> Result<Nip10ReplyAttribution, String> {
    Ok(Nip10ReplyAttribution {
        author_pubkey: attr
            .author_pubkey()
            .ok_or("ReplyAttribution missing author_pubkey")?
            .to_string(),
        reply_event_id: attr
            .reply_event_id()
            .ok_or("ReplyAttribution missing reply_event_id")?
            .to_string(),
        reply_created_at: attr.reply_created_at(),
    })
}

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

#[cfg(test)]
#[path = "typed_wire_tests.rs"]
mod tests;
