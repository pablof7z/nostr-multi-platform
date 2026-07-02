//! `nmp-nip23` — NIP-23 long-form article ownership for NMP.
//!
//! This crate owns the NIP-23 long-form (kind:30023) typed snapshot projection,
//! article feed semantics, addressable-coordinate supersession, NIP-09 delete
//! folding, and the `nmp.nip23.articles` typed projection key.
//!
//! `nmp-content` remains the rendering/tokenization substrate: this crate
//! provides the kind:30023 embed adapter and reuses the `ContentTreeWire` codec
//! for article bodies, but all NIP-23 protocol read-model decisions live here.
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
//! `AppHost::register_typed_snapshot_projection` (ADR-0072). It does **not**
//! emit into the generic JSON `projections` map — that map is being retired.
//!
//! # Reuse, not reinvention
//!
//! The article body shape remains the embed-envelope wire shape in
//! `nmp-content`; this crate owns the kind:30023 field extraction and composes
//! the content renderer to fill the body. The wire codec
//! ([`crate::wire::longform_fb`]) reuses the existing `ContentTreeWire` (`NFCT`)
//! buffer for the article body.
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
//!   `ArticleProjection`), not an app noun. This crate owns it as reusable NMP
//!   protocol infrastructure, never `nmp-core`.
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

use std::sync::Arc;

use nmp_content::embed_projection::{ArticleProjection, ArticleProjectionAdapter};
use nmp_content::wire::ContentTreeWire;
use nmp_core::substrate::{
    KernelEvent, ObservedProjection, ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;

mod feed;
mod installer;
pub mod ownership;
mod projection;
pub mod wire;
pub use feed::{
    longform_acquisition_kinds, longform_feed_predicate, LongformFeed, LongformFeedEntry,
    LongformFeedPredicate, LongformRepostAttribution,
};
pub use installer::{register, Config, Handles};
pub use nmp_kinds::KIND_LONG_FORM_ARTICLE;
pub(crate) use projection::article_address;
pub use projection::{ArticleFeedItem, LongformProjection};

/// Snapshot-projection key the typed sidecar is emitted under.
pub const LONGFORM_PROJECTION_KEY: &str = "nmp.nip23.articles";

/// Owner-provided render adapter for an embedded NIP-23 kind:30023 article.
///
/// Rendering crates may adapt the returned payload into their envelope/wire
/// shape, but the NIP-23 tag semantics stay in this crate.
#[must_use]
pub fn article_embed_projection_from_event(
    event: &KernelEvent,
    content_tree: ContentTreeWire,
) -> Option<ArticleProjection> {
    if event.kind != KIND_LONG_FORM_ARTICLE {
        return None;
    }
    Some(ArticleProjection {
        id: event.id.clone(),
        author_pubkey: event.author.clone(),
        created_at: event.created_at,
        title: tag_value(&event.tags, "title"),
        summary: tag_value(&event.tags, "summary"),
        hero_image_url: tag_value(&event.tags, "image"),
        d_tag: tag_value(&event.tags, "d").unwrap_or_default(),
        content_tree,
    })
}

/// Register this crate's NIP-23 embed adapter with `nmp-content`.
pub fn register_content_embed_projection_adapter() {
    let adapter: ArticleProjectionAdapter = article_embed_projection_from_event;
    nmp_content::register_article_projection_adapter(adapter);
}

/// Wire the default NIP-23 long-form (kind:30023) **typed** snapshot projection
/// into `app`.
///
/// Constructs one [`LongformProjection`] and registers it twice: as an
/// [`ObservedProjectionSink`] and as the typed snapshot projection under
/// [`LONGFORM_PROJECTION_KEY`]. The payload is typed-only (`NL23`) and never
/// writes to the retiring generic JSON `projections` map.
pub(crate) fn register_longform_projection(
    app: &(impl ObservedProjectionRegistrar + SnapshotProjectionRegistrar),
) {
    register_content_embed_projection_adapter();
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
    app.register_typed_snapshot_projection(
        nmp_ownership::DeclaredProjectionKey::framework(
            LONGFORM_PROJECTION_KEY,
            "projection.nmp.nip23.articles",
        ),
        move || Some(projection_for_closure.typed_projection()),
    );
}

fn tag_value(tags: &[Vec<String>], key: &str) -> Option<String> {
    tags.iter()
        .find(|tag| tag.first().is_some_and(|candidate| candidate == key))
        .and_then(|tag| tag.get(1).cloned())
}

#[cfg(test)]
mod tests;
