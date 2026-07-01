//! NIP-23 long-form (kind:30023) **typed** snapshot projection.
//!
//! # Why this module exists — the A5 re-parse pattern
//!
//! The app-conformance scanner found the same defect in THREE apps (Chirp's
//! `EmbedHost`, Podcastr's feedback subsystem, tenex-off's article feed): the
//! kernel emits **raw events** and each consumer re-parses + re-supersedes them
//! in app code because **no typed projection exists** for the kind. For
//! kind:30023 specifically, tenex-off had to register a `RawEventObserver` and
//! run `article_from_event_json` itself, dragging in app-side supersession
//! (`created_at >`). The durable fix is a typed projection in the framework,
//! not in the app.
//!
//! This is that projection. It is a [`ObservedProjectionSink`] with `Mutex<State>`
//! interior mutability whose output is a **typed FlatBuffers sidecar** payload
//! ([`TypedProjectionData`]) registered via
//! `AppHost::register_typed_snapshot_projection` (ADR-0037). It does **not**
//! emit into the generic JSON `projections` map — that map is being retired.
//!
//! # Reuse, not reinvention
//!
//! The typed shape ([`ArticleProjection`]) and the tag-parser
//! ([`resolve_embed_projection`]) already exist in this crate; the wire codec
//! ([`crate::wire::longform_fb`]) reuses the existing `ContentTreeWire` (`NFCT`)
//! buffer for the article body. This module is a thin observer that calls the
//! existing resolver — it does NOT write a new NIP-23 tag parser, a new content
//! renderer, or a new supersession rule.
//!
//! # Supersession — latest-at-coordinate collapse
//!
//! kind:30023 is parameterized-replaceable (30000–39999): the kernel's
//! `EventStore` resolves newest-per-`(author, kind, d-tag)` on insert and fires
//! [`ObservedProjectionSink`] **only on `Inserted | Replaced`**, so in normal kernel
//! delivery a late older arrival never reaches us. But the collapse rule is the
//! *coordinate identity*, not arrival order, and this observer is a public seam.
//! The map keyed by the addressable coordinate therefore keeps the winner by a
//! `created_at` comparison (mirroring `LongformFeed`'s merge): the newest event
//! for a coordinate wins regardless of delivery order, and versions collapse to
//! one row rather than the last writer clobbering a newer one.
//!
//! # D5-bounded — scoped to what's open/claimed
//!
//! A [`ObservedProjectionSink`] only ever sees events the kernel actually
//! subscribed to. The two shapes apps need both arrive on this one stream:
//!
//! * **article feed** — events from an open `topic_articles` (`#t`) interest.
//! * **open document** — events fetched by an event `resolve_ref` claim.
//!
//! There is no unbounded kind:30023 history here; the snapshot only ever holds
//! the articles whose subscriptions are (or were) open this session.
//!
//! # Doctrine map
//!
//! * **D0** — kind:30023 is a NIP-23 *protocol* concept (like the existing
//!   `ArticleProjection`), not an app noun. This module lives in `nmp-content`
//!   (a rendering sidecar), never `nmp-core`.
//! * **D1** — every feed-summary display field is a non-`Option` placeholder
//!   (empty string) rather than an optional that gates rendering, so a missing
//!   `title`/`summary`/`image` does not hide the row. The full document keeps
//!   the resolver's `Option` tag fields verbatim (raw protocol data).
//! * **D5** — the feed list is a *trimmed summary* (no `content_tree`); only the
//!   open-document map carries the full article body, so the per-tick feed
//!   payload stays small and screen-shaped.
//! * **D6** — a poisoned mutex degrades to an empty projection, never a panic
//!   across the snapshot boundary.
//! * **D8** — building the typed payload is a cheap, non-blocking map walk; safe
//!   to run on the actor thread inside the snapshot tick.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{
    KernelEvent, ObservedProjection, ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::{ObservedProjectionSink, TypedProjectionData};
use serde::{Deserialize, Serialize};

use crate::context::RenderContext;
use crate::embed_projection::{resolve_embed_projection, ArticleProjection, EmbedKindProjection};
use crate::wire::longform_fb;

mod feed;
pub use feed::{
    longform_acquisition_kinds, longform_feed_predicate, LongformFeed, LongformFeedEntry,
    LongformFeedPredicate, LongformRepostAttribution,
};
pub use nmp_kinds::KIND_LONG_FORM_ARTICLE;

/// Snapshot-projection key the typed sidecar is emitted under.
pub const LONGFORM_PROJECTION_KEY: &str = "nmp.nip23.articles";

/// Trimmed, screen-shaped summary for an article **feed list** row (D5).
///
/// Deliberately omits the full `content_tree` the open-document shape carries —
/// a feed list never renders the article body. Display fields are non-`Option`
/// placeholders (D1): a missing tag yields an empty string, not a hidden row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArticleFeedItem {
    /// Addressable coordinate `kind:author_hex:d_tag` — the stable identity an
    /// app uses to open the full document.
    pub address: String,
    /// 64-character hex event id of the winning event.
    pub id: String,
    /// 64-character hex author pubkey.
    pub author_pubkey: String,
    /// `title` tag, or empty string when absent (D1 placeholder).
    pub title: String,
    /// `summary` tag, or empty string when absent (D1 placeholder).
    pub summary: String,
    /// `image` (hero) tag, or empty string when absent (D1 placeholder).
    pub hero_image_url: String,
    /// Addressable `d` tag value.
    pub d_tag: String,
    /// Event creation time as Unix seconds.
    pub created_at: u64,
}

impl ArticleFeedItem {
    fn from_article(address: String, article: &ArticleProjection) -> Self {
        Self {
            address,
            id: article.id.clone(),
            author_pubkey: article.author_pubkey.clone(),
            title: article.title.clone().unwrap_or_default(),
            summary: article.summary.clone().unwrap_or_default(),
            hero_image_url: article.hero_image_url.clone().unwrap_or_default(),
            d_tag: article.d_tag.clone(),
            created_at: article.created_at,
        }
    }
}

/// Addressable coordinate for a parameterized-replaceable event:
/// `kind:author_hex:d_tag`. This is the supersession identity — newest event
/// for a given coordinate wins.
///
/// Delegates to [`nmp_nip09::AddressCoordinate`], the single canonical place
/// that computes address-coordinate identity (issue #1740 step 5; ownership
/// widened to `nmp-nip09` by #2589). The `d_tag` passed here comes from the
/// same resolved article projection the row renders, so the wire string stays
/// consistent with the repost `a` tag and the kind:5 tombstone key.
fn article_address(event: &KernelEvent, d_tag: &str) -> String {
    nmp_nip09::AddressCoordinate::new(event.kind, event.author.clone(), d_tag).to_wire()
}

/// Long-form (kind:30023) typed snapshot projection.
///
/// Accumulates the resolved [`ArticleProjection`] for every kind:30023 event the
/// kernel surfaces (open topic feed + claimed documents), keyed by addressable
/// coordinate so the latest event for an `(author, d_tag)` wins. See the module
/// docs for the supersession + D5-scope contract.
#[derive(Default)]
pub struct LongformProjection {
    /// `address (kind:author:d_tag) -> resolved article`. BTreeMap for
    /// deterministic snapshot key ordering (diff stability across ticks, and a
    /// sorted `(key)` documents vector on the wire).
    state: Mutex<BTreeMap<String, ArticleProjection>>,
}

impl LongformProjection {
    /// Construct an empty projection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the typed sidecar entry for the `nmp.nip23.articles` projection.
    ///
    /// Always returns a present [`TypedProjectionData`] — an empty projection is
    /// a well-formed empty buffer (D1: present and typed even when empty). The
    /// `articles` feed list is sorted newest-first; the `documents` map carries
    /// the full [`ArticleProjection`] bodies keyed by addressable coordinate.
    ///
    /// D6: a poisoned mutex degrades to an empty buffer rather than panicking on
    /// the actor thread.
    #[must_use]
    pub fn typed_projection(&self) -> TypedProjectionData {
        let documents = match self.state.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => BTreeMap::new(),
        };
        let mut articles: Vec<ArticleFeedItem> = documents
            .iter()
            .map(|(address, article)| ArticleFeedItem::from_article(address.clone(), article))
            .collect();
        // Newest-first; ties broken by address for deterministic ordering.
        articles.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.address.cmp(&b.address))
        });
        let payload = longform_fb::encode_longform_articles(&articles, &documents);
        TypedProjectionData {
            key: LONGFORM_PROJECTION_KEY.to_string(),
            schema_id: longform_fb::SCHEMA_ID.to_string(),
            schema_version: longform_fb::SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(longform_fb::FILE_IDENTIFIER).into_owned(),
            payload,
            ..Default::default()
        }
    }
}

impl ObservedProjectionSink for LongformProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        // NIP-09: a kind:5 deletion retracts a stored coordinate (issue #1740
        // step 5), so the typed projection cannot keep serving a deleted
        // article after `LongformFeed` drops it.
        if let Some(record) = nmp_nip09::DeleteRecord::try_from_kernel_event(event) {
            self.apply_delete(&record);
            return;
        }
        if event.kind != KIND_LONG_FORM_ARTICLE {
            return;
        }
        // Reuse the existing NIP-23 resolver — never a bespoke tag parser.
        let ctx = RenderContext::default();
        let EmbedKindProjection::Article(article) = resolve_embed_projection(event, &ctx) else {
            // resolve_embed_projection maps 30023 -> Article unconditionally;
            // any other variant means an upstream change. D6: ignore rather
            // than panic.
            return;
        };
        let address = article_address(event, &article.d_tag);
        if let Ok(mut state) = self.state.lock() {
            // Latest-at-coordinate collapse. The kernel normally fires us only
            // with the store's winner (Inserted | Replaced), so arrival order is
            // already chronological — but the observer is a public seam and the
            // collapse rule is the *coordinate identity*, not arrival order. A
            // `created_at` guard makes the winner deterministic regardless of
            // delivery order (mirroring `LongformFeed`'s merge), so a late older
            // event can never clobber the newer version at this coordinate.
            match state.entry(address) {
                std::collections::btree_map::Entry::Occupied(mut slot) => {
                    if article.created_at >= slot.get().created_at {
                        slot.insert(article);
                    }
                }
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(article);
                }
            }
        }
    }
}

impl LongformProjection {
    /// Apply a NIP-09 deletion to the stored documents.
    ///
    /// Mirrors [`LongformFeed`]'s row semantics: an `a`-tag coordinate is
    /// retracted only when the delete author owns it and the stored version was
    /// created at or before the deletion; an `e`-tag retracts the coordinate
    /// whose winning event id matches and whose author owns it. Unresolvable and
    /// foreign targets are no-ops — never guess a coordinate.
    fn apply_delete(&self, record: &nmp_nip09::DeleteRecord) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        for coord in &record.address_targets {
            if coord.pubkey != record.author {
                continue;
            }
            let key = coord.to_wire();
            let retract = state
                .get(&key)
                .is_some_and(|article| article.created_at <= record.created_at);
            if retract {
                state.remove(&key);
            }
        }
        if !record.event_targets.is_empty() {
            state.retain(|_, article| {
                !(record.event_targets.contains(&article.id)
                    && article.author_pubkey == record.author)
            });
        }
    }
}

/// Wire the default NIP-23 long-form (kind:30023) **typed** snapshot projection
/// into `app`.
///
/// Constructs one [`LongformProjection`] and registers it twice: as an
/// [`ObservedProjectionSink`] and as the typed snapshot projection under
/// [`LONGFORM_PROJECTION_KEY`]. The payload is typed-only (`NL23`) and never
/// writes to the retiring generic JSON `projections` map.
pub fn register_longform_projection(
    app: &(impl ObservedProjectionRegistrar + SnapshotProjectionRegistrar),
) {
    let projection = Arc::new(LongformProjection::new());
    let observer_id = app.open_observed_projection(ObservedProjection::from_kinds(
        Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>,
        LONGFORM_PROJECTION_KEY,
        1,
        [KIND_LONG_FORM_ARTICLE],
        512,
    ));
    if observer_id == nmp_core::ObservedProjectionId(0) {
        return;
    }
    let projection_for_closure = Arc::clone(&projection);
    app.register_typed_snapshot_projection(LONGFORM_PROJECTION_KEY, move || {
        Some(projection_for_closure.typed_projection())
    });
}

#[cfg(test)]
mod tests;
