//! Long-form article feed adapter over generic `nmp-feed` mechanics.
//!
//! Apps declare primary kind `30023`; this adapter derives kind:16 acquisition
//! at the protocol/content layer and admits direct articles plus NIP-18 generic
//! repost wrappers. It never fetches profiles, reply counts, thread roots, or
//! missing target events. Tag-only wrappers can hydrate only from the injected
//! local [`EventLookup`].

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_feed::{
    EventLookup, FeedRequest, FlatFeed as GenericFlatFeed, FlatFeedItem, FlatFeedItemBuilder,
    FlatFeedMerge, RootFeedSnapshot,
};
use serde::{Deserialize, Serialize};

use nmp_content::context::RenderContext;
use nmp_content::embed_projection::{resolve_embed_projection, EmbedKindProjection};

use crate::{article_address, ArticleFeedItem, KIND_LONG_FORM_ARTICLE};

/// Admission predicate supplied by the app/protocol composition layer.
pub type LongformFeedPredicate = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// Repost attribution for a long-form feed row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongformRepostAttribution {
    /// Pubkey of the account that authored the kind:16 wrapper.
    pub author_pubkey: String,
    /// Event id of the kind:16 wrapper.
    pub repost_event_id: String,
    /// Wrapper timestamp used for feed ordering.
    pub repost_created_at: u64,
}

/// Renderable long-form feed row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongformFeedEntry {
    /// Canonical row id: target article address (`kind:author:d`), not wrapper id.
    pub id: String,
    /// Article summary when the target is direct, embedded, or locally known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub article: Option<ArticleFeedItem>,
    /// Wrapper attribution when this row is currently positioned by a repost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reposted_by: Option<LongformRepostAttribution>,
    /// Relay provenance from the source event that positioned the row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relay_provenance: Vec<String>,
}

/// A flat feed for NIP-23 long-form articles and kind:16 repost wrappers.
pub struct LongformFeed {
    inner: Arc<GenericFlatFeed<LongformFeedEntry>>,
}

impl LongformFeed {
    /// Construct a feed without local target lookup.
    #[must_use]
    pub fn new(predicate: LongformFeedPredicate) -> Arc<Self> {
        Self::with_event_lookup(predicate, Arc::new(|_| None))
    }

    /// Construct a feed with an injected local target lookup.
    #[must_use]
    pub fn with_event_lookup(
        predicate: LongformFeedPredicate,
        event_lookup: EventLookup,
    ) -> Arc<Self> {
        Self::with_options(predicate, event_lookup, None)
    }

    /// Construct a topic-scoped feed.
    ///
    /// Subscription composition must use separate direct/repost acquisition
    /// lanes because one Nostr filter cannot express
    /// `(kind:30023 AND #t=topic) OR (kind:16 AND #k=30023)`.
    ///
    /// Direct articles must carry `#t=<topic>`. Reposts must embed or locally
    /// resolve a target article with that tag; tag-only unresolved reposts are
    /// ignored because the feed cannot prove they belong to the topic without
    /// fetching the target.
    #[must_use]
    pub fn for_topic(
        topic: impl Into<String>,
        predicate: LongformFeedPredicate,
        event_lookup: EventLookup,
    ) -> Arc<Self> {
        Self::with_options(predicate, event_lookup, Some(topic.into()))
    }

    fn with_options(
        predicate: LongformFeedPredicate,
        event_lookup: EventLookup,
        topic: Option<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: GenericFlatFeed::with_merge(
                predicate,
                longform_item_builder(event_lookup, topic),
                None,
                longform_merge(),
            ),
        })
    }

    /// Build a newest-first snapshot over the visible rows.
    #[must_use]
    pub fn snapshot(&self, request: &FeedRequest) -> RootFeedSnapshot<LongformFeedEntry, ()> {
        self.inner.snapshot(request)
    }

    /// Return whether the feed currently has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Return the number of canonical rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl ObservedProjectionSink for LongformFeed {
    fn on_kernel_event(&self, event: &KernelEvent) {
        // NIP-09: a kind:5 deletion suppresses rows it can prove (issue #1740
        // step 5). Apply it before normal ingest; a non-delete event flows to
        // the inner feed unchanged.
        if let Some(record) = nmp_nip09::DeleteRecord::try_from_kernel_event(event) {
            self.apply_delete(&record);
            return;
        }
        self.inner.on_kernel_event(event);
    }
}

impl LongformFeed {
    /// Apply a NIP-09 deletion to the feed rows.
    ///
    /// * An `a`-tag target removes the coordinate-keyed row — but only when the
    ///   delete author owns that coordinate (the coordinate pubkey), so a
    ///   foreign delete can never remove someone else's article, and only when
    ///   the row's known version was created at or before the deletion
    ///   (`created_at <= delete.created_at`), so a newer version published after
    ///   the retraction survives. A body-less coordinate row (no known version)
    ///   cannot be proven newer, so the deletion removes it.
    /// * An `e`-tag target removes any source contribution the delete author
    ///   owns that names that event id — either the wrapper/article event the
    ///   source was built from, OR the **deleted article body** itself when it
    ///   is surfaced through someone else's repost. The latter stops a retracted
    ///   article from living on through a repost: A's `e:art` delete clears A's
    ///   article from C's repost row even though C's wrapper id differs.
    /// * An unresolvable target removes nothing — never guess a coordinate.
    fn apply_delete(&self, record: &nmp_nip09::DeleteRecord) {
        for coord in &record.address_targets {
            if coord.pubkey != record.author {
                continue;
            }
            let key = coord.to_wire();
            // Remove the retracted versions at the SOURCE level, not just the
            // best card: a row keyed at this coordinate can hold several source
            // contributions (the direct article plus reposts). Dropping only the
            // best row would leave a stale source behind that a later removal of
            // the surviving source could reanimate. `remove_sources_if`
            // recomputes the row from whatever survives (or drops it if empty).
            self.inner.remove_sources_if(|item| {
                item.id == key
                    && item
                        .card
                        .article
                        .as_ref()
                        .is_none_or(|article| article.created_at <= record.created_at)
            });
        }
        for event_id in &record.event_targets {
            self.inner
                .remove_sources_if(|item| e_tag_delete_matches(item, event_id, &record.author));
        }
    }
}

/// Whether an `e`-tag delete by `author` targeting `event_id` removes this
/// source contribution (NIP-09 — only the delete author's own events).
///
/// Matches two ownership-validated shapes:
/// * the source event itself (`source_id == event_id`) is the delete author's —
///   a direct article row, or a repost wrapper authored by the deleter; or
/// * the source's rendered **article body** is the deleted event
///   (`article.id == event_id`) and that article is the delete author's — so a
///   retracted article is dropped even when it is surfaced through another
///   account's repost (whose wrapper id differs from the deleted article id).
fn e_tag_delete_matches(
    item: &FlatFeedItem<LongformFeedEntry>,
    event_id: &str,
    author: &str,
) -> bool {
    let source_owned_by_author = match item.card.reposted_by.as_ref() {
        Some(repost) => repost.author_pubkey == author,
        None => item
            .card
            .article
            .as_ref()
            .is_some_and(|article| article.author_pubkey == author),
    };
    if item.source_id == event_id && source_owned_by_author {
        return true;
    }
    item.card
        .article
        .as_ref()
        .is_some_and(|article| article.id == event_id && article.author_pubkey == author)
}

/// Compile kind:30023 primary feed acquisition kinds.
#[must_use]
pub fn longform_acquisition_kinds() -> BTreeSet<u32> {
    match nmp_nip18::try_acquisition_kinds_for_primary([KIND_LONG_FORM_ARTICLE]) {
        Ok(kinds) => kinds,
        Err(_) => [KIND_LONG_FORM_ARTICLE].into_iter().collect(),
    }
}

/// Build a source-event predicate for a long-form feed.
///
/// For a direct article the source is the article author. For a repost wrapper
/// the source is the reposter; target authors are not used for admission.
#[must_use]
pub fn longform_feed_predicate(
    source_allows: Arc<dyn Fn(&str) -> bool + Send + Sync>,
) -> LongformFeedPredicate {
    Arc::new(move |event: &KernelEvent| {
        (event.kind == KIND_LONG_FORM_ARTICLE || event.kind == nmp_nip18::KIND_GENERIC_REPOST)
            && source_allows(&event.author)
    })
}

fn longform_item_builder(
    event_lookup: EventLookup,
    topic: Option<String>,
) -> FlatFeedItemBuilder<LongformFeedEntry> {
    Arc::new(move |event| match event.kind {
        KIND_LONG_FORM_ARTICLE => article_item_from_target(event, topic.as_deref()),
        nmp_nip18::KIND_GENERIC_REPOST => {
            article_item_from_repost(event, &event_lookup, topic.as_deref())
        }
        _ => None,
    })
}

fn article_item_from_target(
    event: &KernelEvent,
    topic: Option<&str>,
) -> Option<FlatFeedItem<LongformFeedEntry>> {
    let article = article_summary_from_event(event)?;
    let id = article.address.clone();
    if topic.is_some_and(|topic| !event_has_topic(event, topic)) {
        return None;
    }
    Some(FlatFeedItem {
        id: id.clone(),
        source_id: event.id.clone(),
        sort_created_at: article.created_at,
        card: LongformFeedEntry {
            id,
            article: Some(article),
            reposted_by: None,
            relay_provenance: event.relay_provenance.clone(),
        },
    })
}

fn article_item_from_repost(
    event: &KernelEvent,
    event_lookup: &EventLookup,
    topic: Option<&str>,
) -> Option<FlatFeedItem<LongformFeedEntry>> {
    let record = nmp_nip18::try_from_kernel_event(event)?;
    let article = article_from_repost_target(&record, event, event_lookup, topic);

    // Row identity is the address coordinate. Prefer the resolved article's
    // coordinate; otherwise use the wrapper's proven `a`-tag/embedded coordinate
    // (issue #1740 step 5: the coordinate is the canonical identity). A wrapper
    // that carries only an event id proves no coordinate, so it stays
    // UNRESOLVED and never positions a row — never guess one from an event id.
    let coordinate = article
        .as_ref()
        .map(|article| article.address.clone())
        .or_else(|| longform_target_coordinate(&record).map(|coord| coord.to_wire()))?;

    // A topic feed cannot prove topic membership for a body-less coordinate row;
    // only admit it once a target article (embedded/local) confirms the topic.
    if topic.is_some() && article.is_none() {
        return None;
    }

    Some(FlatFeedItem {
        id: coordinate.clone(),
        source_id: event.id.clone(),
        sort_created_at: event.created_at,
        card: LongformFeedEntry {
            id: coordinate,
            article,
            reposted_by: Some(repost_attribution(event)),
            relay_provenance: event.relay_provenance.clone(),
        },
    })
}

/// The repost's proven target coordinate, restricted to long-form (kind:30023).
///
/// A generic repost can wrap any addressable kind; this feed only positions a
/// row for a kind:30023 target. Returns `None` for a non-30023 coordinate or an
/// event-id-only wrapper (no proven coordinate).
fn longform_target_coordinate(
    record: &nmp_nip18::RepostRecord,
) -> Option<nmp_nip09::AddressCoordinate> {
    record
        .target_address
        .clone()
        .filter(|coord| coord.kind == KIND_LONG_FORM_ARTICLE)
}

fn article_from_repost_target(
    record: &nmp_nip18::RepostRecord,
    event: &KernelEvent,
    event_lookup: &EventLookup,
    topic: Option<&str>,
) -> Option<ArticleFeedItem> {
    if let Some(embedded) = record.embedded_event.clone() {
        let target_event = KernelEvent {
            id: embedded.id,
            author: embedded.author,
            kind: embedded.kind,
            created_at: embedded.created_at,
            tags: embedded.tags,
            content: embedded.content,
            relay_provenance: event.relay_provenance.clone(),
        };
        if topic.is_some_and(|topic| !event_has_topic(&target_event, topic)) {
            return None;
        }
        return article_summary_from_event(&target_event);
    }
    record
        .target_event_id
        .as_ref()
        .and_then(|target_id| (event_lookup)(target_id))
        .and_then(|target| {
            if topic.is_some_and(|topic| !event_has_topic(&target, topic)) {
                None
            } else {
                article_summary_from_event(&target)
            }
        })
}

fn article_summary_from_event(event: &KernelEvent) -> Option<ArticleFeedItem> {
    if event.kind != KIND_LONG_FORM_ARTICLE {
        return None;
    }
    let ctx = RenderContext::default();
    let EmbedKindProjection::Article(article) = resolve_embed_projection(event, &ctx) else {
        return None;
    };
    Some(ArticleFeedItem::from_article(
        article_address(event, &article.d_tag),
        &article,
    ))
}

fn event_has_topic(event: &KernelEvent, topic: &str) -> bool {
    event.tags.iter().any(|tag| {
        tag.first().is_some_and(|name| name == "t")
            && tag.get(1).is_some_and(|value| value == topic)
    })
}

fn repost_attribution(event: &KernelEvent) -> LongformRepostAttribution {
    LongformRepostAttribution {
        author_pubkey: event.author.clone(),
        repost_event_id: event.id.clone(),
        repost_created_at: event.created_at,
    }
}

fn longform_merge() -> FlatFeedMerge<LongformFeedEntry> {
    Arc::new(|existing, incoming| match existing {
        Some(existing) => merge_longform_sources(existing, incoming),
        None => incoming,
    })
}

fn merge_longform_sources(
    existing: &FlatFeedItem<LongformFeedEntry>,
    incoming: FlatFeedItem<LongformFeedEntry>,
) -> FlatFeedItem<LongformFeedEntry> {
    let incoming_wins = incoming.sort_created_at > existing.sort_created_at
        || (incoming.sort_created_at == existing.sort_created_at
            && incoming.card.article.is_some()
            && existing.card.article.is_none());
    let (mut best, other) = if incoming_wins {
        (incoming, existing.clone())
    } else {
        (existing.clone(), incoming)
    };

    if let Some(article) = freshest_article(best.card.article.as_ref(), other.card.article.as_ref())
    {
        best.card.article = Some(article);
    }
    if best.card.relay_provenance.is_empty() {
        best.card.relay_provenance = other.card.relay_provenance;
    }
    best
}

fn freshest_article(
    left: Option<&ArticleFeedItem>,
    right: Option<&ArticleFeedItem>,
) -> Option<ArticleFeedItem> {
    match (left, right) {
        (Some(left), Some(right)) if right.created_at > left.created_at => Some(right.clone()),
        (Some(left), _) => Some(left.clone()),
        (None, Some(right)) => Some(right.clone()),
        (None, None) => None,
    }
}

#[cfg(test)]
#[path = "feed_tests.rs"]
mod tests;
