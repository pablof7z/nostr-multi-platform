//! Protocol composition: build a generic [`nmp_feed::FeedRow`] from NIP-01 /
//! NIP-18 facts on the four generic [`nmp_feed::FlatFeed`] knobs.
//!
//! This crate no longer owns a feed ENGINE or an authoritative row type. The
//! engine is the generic `nmp_feed::FlatFeed<FeedRow>`; the row is the generic
//! `nmp_feed::FeedRow`. This module only supplies the app/protocol knobs:
//!
//!   * admission — supplied by the caller (a `nmp_feed::FlatFeedPredicate`);
//!   * identity  — [`feed_row_builder`] keys a NIP-18 repost by its TARGET id
//!                 (`nmp-nip18` parse), defaulting to `event.id` otherwise;
//!   * sort      — `sort_created_at = event.created_at` (a repost bumps the row);
//!   * merge     — [`timeline_merge`] folds a repost wrapper and the target's own
//!                 event into one row, preserving repost provenance.
//!
//! The former note-only magic (NIP-10 root promotion, reply rollup, repost L-2
//! rekey, synchronous `EventLookup` cache reads — the order-dependent cache-luck
//! bug #3083) is DELETED. Reposts carry a typed [`nmp_feed::RenderTarget`]
//! pointer resolved lazily via `resolve_ref`, not by an in-feed cache read.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_feed::{
    FeedRow, FeedRowContext, FlatFeedItem, FlatFeedItemBuilder, FlatFeedMerge, FlatFeedPredicate,
    RenderTarget,
};
use nmp_planner::InterestShape;

/// Source-owned provider of NIP-29 hosted-group context for a feed row.
///
/// Returns a [`FeedRowContext::Group`] (carried as data). The canonical typed
/// group id is `nmp_nip29::GroupId`, owned by `nmp-nip29`; it is intentionally
/// NOT duplicated as a typed struct in the low `nmp-feed` row crate.
pub type FeedRowGroupContext = Arc<dyn Fn(&KernelEvent) -> Option<FeedRowContext> + Send + Sync>;

/// An empty group-context provider.
#[must_use]
pub fn no_group_context() -> FeedRowGroupContext {
    Arc::new(|_| None)
}

/// Build the item-builder knob: `KernelEvent -> Option<FlatFeedItem<FeedRow>>`.
///
/// NIP-18 reposts key the row by the TARGET id and carry a render-target
/// pointer + repost provenance context. When the wrapper embeds the target JSON
/// (NIP-18 kind:6 `content`), the row is hydrated from that embedded event —
/// this is parsing the wrapper's own payload, NOT a cache read.
///
/// TODO(#3082): whether the embedded target JSON is ALSO ingested into the
/// kernel event store at parse time (so `render_target` cache-serves for OTHER
/// surfaces) is an open decision. Not implemented here — bare reposts resolve
/// their target lazily via `resolve_ref`.
#[must_use]
pub fn feed_row_builder(group_context: FeedRowGroupContext) -> FlatFeedItemBuilder<FeedRow> {
    Arc::new(move |event: &KernelEvent| Some(build_item(event, group_context(event))))
}

fn build_item(event: &KernelEvent, group: Option<FeedRowContext>) -> FlatFeedItem<FeedRow> {
    match nmp_nip18::try_from_kernel_event(event) {
        Some(repost) => build_repost_item(event, &repost, group),
        None => {
            let mut row = FeedRow::from_event(event);
            if let Some(group) = group {
                row.context.push(group);
            }
            FlatFeedItem {
                id: row.id.clone(),
                source_id: event.id.clone(),
                sort_created_at: row.created_at,
                card: row,
            }
        }
    }
}

fn build_repost_item(
    event: &KernelEvent,
    repost: &nmp_nip18::RepostRecord,
    group: Option<FeedRowContext>,
) -> FlatFeedItem<FeedRow> {
    let target_id = repost
        .target_event_id
        .clone()
        .unwrap_or_else(|| event.id.clone());
    let render_target = Some(RenderTarget::Event {
        id: target_id.clone(),
        relay: None,
        event_kind: repost.target_kind,
    });

    let (author_pubkey, kind, content, tags, note_created_at) = match repost.embedded_event.as_ref()
    {
        Some(inner) => (
            inner.author.clone(),
            inner.kind,
            inner.content.clone(),
            inner.tags.clone(),
            inner.created_at,
        ),
        // No embedded JSON: placeholder row. The render-target pointer drives a
        // lazy `resolve_ref`; when the target's own event is admitted it merges
        // in (see `timeline_merge`).
        None => (
            String::new(),
            repost.target_kind.unwrap_or(0),
            String::new(),
            Vec::new(),
            event.created_at,
        ),
    };

    let mut context = vec![FeedRowContext::Repost {
        author_pubkey: event.author.clone(),
        note_created_at,
    }];
    if let Some(group) = group {
        context.push(group);
    }

    let row = FeedRow {
        id: target_id.clone(),
        source_id: event.id.clone(),
        author_pubkey,
        kind,
        created_at: event.created_at,
        content,
        tags,
        relay_provenance: event.received_from_relays(),
        render_target,
        context,
    };
    FlatFeedItem {
        id: target_id,
        source_id: event.id.clone(),
        // A repost bumps the shared row to the repost time.
        sort_created_at: event.created_at,
        card: row,
    }
}

/// The follows-timeline merge knob: when a repost wrapper and the target's own
/// event map to the same row id, keep the bumped sort position and repost
/// provenance while preferring the hydrated (non-placeholder) content.
#[must_use]
pub fn timeline_merge() -> FlatFeedMerge<FeedRow> {
    Arc::new(|existing, incoming| match existing {
        Some(existing) if existing.sort_created_at > incoming.sort_created_at => {
            merge_target_into_bumped_row(existing, incoming)
        }
        Some(existing) if existing.sort_created_at == incoming.sort_created_at => {
            prefer_hydrated(existing, incoming)
        }
        _ => incoming,
    })
}

fn merge_target_into_bumped_row(
    existing: &FlatFeedItem<FeedRow>,
    incoming: FlatFeedItem<FeedRow>,
) -> FlatFeedItem<FeedRow> {
    let Some(repost) = repost_context(&existing.card) else {
        return existing.clone();
    };
    if repost_context(&incoming.card).is_some() {
        // Both are reposts; keep the newer (existing sorts higher).
        return existing.clone();
    }
    // `incoming` is the plain target event: take its hydrated fields but keep
    // the bumped sort position and re-attach repost provenance.
    let mut card = incoming.card;
    card.context.push(repost);
    card.render_target = existing.card.render_target.clone();
    FlatFeedItem {
        id: existing.id.clone(),
        source_id: existing.source_id.clone(),
        sort_created_at: existing.sort_created_at,
        card,
    }
}

fn prefer_hydrated(
    existing: &FlatFeedItem<FeedRow>,
    incoming: FlatFeedItem<FeedRow>,
) -> FlatFeedItem<FeedRow> {
    if is_placeholder(&existing.card) && !is_placeholder(&incoming.card) {
        return FlatFeedItem {
            id: incoming.id,
            source_id: existing.source_id.clone(),
            sort_created_at: existing.sort_created_at,
            card: incoming.card,
        };
    }
    incoming
}

fn repost_context(card: &FeedRow) -> Option<FeedRowContext> {
    card.context
        .iter()
        .find(|ctx| matches!(ctx, FeedRowContext::Repost { .. }))
        .cloned()
}

fn is_placeholder(card: &FeedRow) -> bool {
    card.content.is_empty() && repost_context(card).is_some()
}

// ── Author / thread flat-feed predicates and shapes (generic protocol knobs) ──

/// Author-feed pull interest: `{authors:[pk], kinds}`.
#[must_use]
pub fn author_feed_shape(author: String, kinds: Vec<u32>) -> InterestShape {
    InterestShape::timeline_for(
        BTreeSet::from([author]),
        kinds.into_iter().collect::<BTreeSet<u32>>(),
    )
}

/// Thread-feed reply-tail pull interest: `{kinds, #e:[root]}`.
#[must_use]
pub fn thread_feed_shape(root_id: String, kinds: Vec<u32>) -> InterestShape {
    let mut shape = InterestShape {
        kinds: kinds.into_iter().collect::<BTreeSet<u32>>(),
        ..Default::default()
    };
    shape
        .tags
        .insert("e".to_string(), BTreeSet::from([root_id]));
    shape
}

/// Author-feed admission predicate: a kind set authored by one pubkey.
#[must_use]
pub fn author_feed_predicate(author: String, kinds: Vec<u32>) -> FlatFeedPredicate {
    Arc::new(move |event: &KernelEvent| event.author == author && kinds.contains(&event.kind))
}

/// Thread-feed admission predicate: the root note itself plus every event of a
/// host-chosen kind that references the root via a NIP-10 `#e` tag.
#[must_use]
pub fn thread_feed_predicate(root_id: String, kinds: Vec<u32>) -> FlatFeedPredicate {
    Arc::new(move |event: &KernelEvent| {
        if event.id == root_id {
            return true;
        }
        if !kinds.contains(&event.kind) {
            return false;
        }
        event
            .tags
            .iter()
            .any(|tag| tag.first().map(String::as_str) == Some("e") && tag.get(1) == Some(&root_id))
    })
}
