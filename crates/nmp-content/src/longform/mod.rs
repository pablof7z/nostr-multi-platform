//! NIP-23 long-form (kind:30023) typed snapshot projection.
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
//! This is that projection. It is a **peer of the other default projections**
//! (`claimed_events`, `nmp.wot.bootstrap`, `nmp.nip17.dm_inbox`): a
//! [`KernelEventObserver`] with `Mutex<State>` interior mutability and a
//! [`snapshot_json`](LongformProjection::snapshot_json) method, registered via
//! `register_snapshot_projection` in `nmp-app-template::register_defaults`. It
//! mirrors `nmp-wot`'s `WotBootstrapRuntime` exactly.
//!
//! # Reuse, not reinvention
//!
//! The typed shape ([`ArticleProjection`]) and the tag-parser
//! ([`resolve_embed_projection`]) already exist in this crate. This module is a
//! thin observer that calls the existing resolver — it does NOT write a new
//! NIP-23 tag parser.
//!
//! # Supersession is free (do NOT reimplement "is this newer?")
//!
//! kind:30023 is parameterized-replaceable (30000–39999): the kernel's
//! `EventStore` resolves newest-per-`(author, kind, d-tag)` on insert and fires
//! [`KernelEventObserver`] **only on `Inserted | Replaced`**. A late older
//! arrival is rejected by the store → the observer never fires → our map keeps
//! the winner. So a map keyed by the addressable coordinate with plain
//! last-write-wins converges to exactly the store's winning event, with **no
//! `created_at` comparison in this module**.
//!
//! # D5-bounded — scoped to what's open/claimed
//!
//! A [`KernelEventObserver`] only ever sees events the kernel actually
//! subscribed to. The two shapes apps need both arrive on this one stream:
//!
//! * **article feed** — events from an open `topic_articles` (`#t`) interest.
//! * **open document** — events fetched by a `claim_event(naddr)` claim.
//!
//! There is no unbounded kind:30023 history here; the snapshot only ever holds
//! the articles whose subscriptions are (or were) open this session.
//!
//! # Doctrine map
//!
//! * **D0** — kind:30023 is a NIP-23 *protocol* concept (like the existing
//!   `ArticleProjection`), not an app noun. This module lives in `nmp-content`
//!   (a rendering sidecar), never `nmp-core`. No app nouns cross the boundary.
//! * **D1** — every feed-summary display field is a non-`Option` placeholder
//!   (`String::new()` / `0`) rather than an optional that gates rendering, so a
//!   missing `title`/`summary`/`image` does not hide the row. The full document
//!   keeps the resolver's `Option` tag fields verbatim (raw protocol data).
//! * **D5** — the feed list is a *trimmed summary* (no `content_tree`); only
//!   the open-document map carries the full article body, so the per-tick feed
//!   payload stays small and screen-shaped.
//! * **D6** — a poisoned mutex degrades to an empty projection (`{}`), never a
//!   panic across the snapshot boundary.
//! * **D8** — `snapshot_json` is a cheap, non-blocking map walk; safe to run on
//!   the actor thread inside the snapshot tick.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;

use crate::context::RenderContext;
use crate::embed_projection::{resolve_embed_projection, ArticleProjection, EmbedKindProjection};

/// NIP-23 long-form article kind.
pub const KIND_LONG_FORM_ARTICLE: u32 = 30_023;

/// Snapshot-projection key apps read off each frame's `projections` map.
pub const LONGFORM_PROJECTION_KEY: &str = "nmp.nip23.articles";

/// Trimmed, screen-shaped summary for an article **feed list** row (D5).
///
/// Deliberately omits the full `content_tree` the open-document shape carries —
/// a feed list never renders the article body. Display fields are non-`Option`
/// placeholders (D1): a missing tag yields an empty string, not a hidden row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
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

/// The serialized snapshot value under [`LONGFORM_PROJECTION_KEY`].
///
/// * `articles` — the feed list, trimmed summaries sorted newest-first.
/// * `documents` — full [`ArticleProjection`]s keyed by addressable coordinate,
///   for the open-document shape (carries the `content_tree` body).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LongformSnapshot {
    articles: Vec<ArticleFeedItem>,
    documents: BTreeMap<String, ArticleProjection>,
}

/// Addressable coordinate for a parameterized-replaceable event:
/// `kind:author_hex:d_tag`. This is the supersession identity — newest event
/// for a given coordinate wins.
fn article_address(event: &KernelEvent, d_tag: &str) -> String {
    format!("{}:{}:{}", event.kind, event.author, d_tag)
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
    /// deterministic snapshot key ordering (diff stability across ticks).
    state: Mutex<BTreeMap<String, ArticleProjection>>,
}

impl LongformProjection {
    /// Construct an empty projection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the snapshot `Value` apps read under [`LONGFORM_PROJECTION_KEY`].
    ///
    /// D6: a poisoned mutex degrades to an empty object rather than panicking on
    /// the actor thread.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        let Ok(documents) = self.state.lock() else {
            return serde_json::json!({ "articles": [], "documents": {} });
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
        let snapshot = LongformSnapshot {
            articles,
            documents: documents.clone(),
        };
        serde_json::to_value(snapshot)
            .unwrap_or_else(|_| serde_json::json!({ "articles": [], "documents": {} }))
    }
}

impl KernelEventObserver for LongformProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
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
            // Last-write-wins. The kernel only fired us because the store
            // accepted this event as Inserted | Replaced (the new winner for
            // this coordinate), so plain overwrite == the store's winner. No
            // `created_at` comparison here by design (see module docs).
            state.insert(address, article);
        }
    }
}

#[cfg(test)]
mod tests;
