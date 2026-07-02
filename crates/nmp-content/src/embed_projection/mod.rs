//! Kind-dispatched embed projection (F-CR-01).
//!
//! This module is the single place in the workspace that performs the
//! `match event.kind` decision for content rendering of embedded events.
//! It produces typed `EmbedKindProjection` variants that native registries
//! consume via `EmbeddedEventEnvelope` on the wire.
//!
//! D0-clean: lives in nmp-content (a rendering sidecar), not nmp-core substrate.
//! See ADR-0072 (`docs/decisions/0072-runtime-capability-and-shell-boundary.md`) for the full contract.

use std::sync::OnceLock;

mod derived;
mod envelope;
mod variants;

#[cfg(test)]
mod tests;

pub use derived::{derive_ref_event_envelopes, derive_ref_event_store_envelopes};
pub use envelope::{EmbeddedEventEnvelope, RenderContextWire};
pub use variants::{
    ArticleProjection, EmbedKindProjection, HighlightProjection, ProfileProjection,
    ShortNoteProjection, UnknownProjection,
};

use nmp_core::substrate::KernelEvent;
use nmp_kinds::{
    KIND_HIGHLIGHT, KIND_LONG_FORM_ARTICLE, KIND_PROFILE_METADATA, KIND_SHORT_TEXT_NOTE,
};

use crate::context::RenderContext;
use crate::mode::RenderMode;
use crate::tokenize_with_kind;
use crate::wire::ContentTreeWire;

/// Owner-provided adapter for a NIP-23 kind:30023 article embed.
///
/// `nmp-content` owns the rendering envelope and content tree; the protocol
/// owner owns article tag/content semantics and returns the typed payload to
/// place inside the envelope.
pub type ArticleProjectionAdapter =
    fn(event: &KernelEvent, content_tree: ContentTreeWire) -> Option<ArticleProjection>;

static ARTICLE_PROJECTION_ADAPTER: OnceLock<ArticleProjectionAdapter> = OnceLock::new();

/// Register the NIP-23 owner adapter used for kind:30023 article embeds.
///
/// This is intentionally a registration seam instead of a direct dependency on
/// `nmp-nip23`: `nmp-nip23` already composes the content renderer for its typed
/// long-form projection, so a direct dependency in this crate would create a
/// cycle. Re-registering is idempotent after the first adapter wins.
pub fn register_article_projection_adapter(adapter: ArticleProjectionAdapter) {
    let _ = ARTICLE_PROJECTION_ADAPTER.set(adapter);
}

/// Resolve a known event into the correct `EmbedKindProjection` variant.
/// This is the single `match event.kind` dispatch point for embed content
/// rendering in the entire workspace.
///
/// For the initial cut, author metadata (display_name, picture) is left as
/// `None`. Callers (higher layers or platform registries) enrich from their
/// profile cache / kind:0 projections. This keeps the resolver pure and D0-clean.
///
/// `content_tree` is always produced via the existing tokenizer so that
/// embedded events benefit from the same rich rendering as top-level content.
pub fn resolve_embed_projection(event: &KernelEvent, _ctx: &RenderContext) -> EmbedKindProjection {
    // Always produce a content tree for the embedded event's content.
    // We use Auto mode so kind:30023 articles etc. get the right treatment.
    let tree = tokenize_with_kind(&event.content, &event.tags, RenderMode::Auto, event.kind);
    let content_tree: ContentTreeWire = tree.to_wire();

    // Extract common fields that many variants share.
    let id = event.id.clone();
    let author_pubkey = event.author.clone();
    let created_at = event.created_at;

    // Helper to pull first value for a tag key (e.g. "d", "title", "image").
    let tag_value = |key: &str| -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.first().map(|s| s == key).unwrap_or(false))
            .and_then(|t| t.get(1).cloned())
    };

    match event.kind {
        KIND_PROFILE_METADATA => {
            // Profile (kind:0) semantics belong to the NIP-01 owner. This crate
            // only adapts the owner projection into the embed wire shape.
            let Some(profile) = nmp_nip01::profile_metadata_projection_from_event(event) else {
                return EmbedKindProjection::Profile(ProfileProjection {
                    pubkey: event.author.clone(),
                    display_name: None,
                    picture_url: None,
                    about: None,
                    nip05: None,
                    lud16: None,
                    banner_url: None,
                });
            };
            EmbedKindProjection::Profile(ProfileProjection {
                pubkey: profile.pubkey,
                display_name: profile.display_name,
                picture_url: profile.picture_url,
                about: profile.about,
                nip05: profile.nip05,
                lud16: profile.lud16,
                banner_url: profile.banner_url,
            })
        }
        KIND_SHORT_TEXT_NOTE => {
            // Short note
            // Media extraction is a best-effort helper (URLs that look like media).
            // For a more complete implementation this can delegate to an existing
            // media classifier in the crate.
            let media_urls: Vec<String> = extract_top_level_media(&event.content);

            EmbedKindProjection::ShortNote(ShortNoteProjection {
                id,
                author_pubkey,
                created_at,
                content_tree,
                media_urls,
            })
        }
        KIND_HIGHLIGHT => {
            // Highlight (kind:9802) semantics belong to the NIP-84 owner. This
            // crate only adapts the owner projection into the embed wire shape.
            let Some(highlight) = nmp_nip84::highlight_projection_from_event(event) else {
                return EmbedKindProjection::Highlight(HighlightProjection {
                    id,
                    author_pubkey,
                    created_at,
                    highlighted_text: event.content.clone(),
                    source_event_id: None,
                    source_event_addr: None,
                    source_url: None,
                    context: None,
                });
            };
            EmbedKindProjection::Highlight(HighlightProjection {
                id: highlight.id,
                author_pubkey: highlight.author_pubkey,
                created_at: highlight.created_at,
                highlighted_text: highlight.highlighted_text,
                source_event_id: highlight.source_event_id,
                source_event_addr: highlight.source_event_addr,
                source_url: highlight.source_url,
                context: highlight.context,
            })
        }
        KIND_LONG_FORM_ARTICLE => ARTICLE_PROJECTION_ADAPTER
            .get()
            .and_then(|adapter| adapter(event, content_tree.clone()))
            .map(EmbedKindProjection::Article)
            .unwrap_or_else(|| {
                let alt_text = tag_value("alt");
                EmbedKindProjection::Unknown(UnknownProjection {
                    kind: event.kind,
                    author_pubkey,
                    created_at,
                    content: event.content.clone(),
                    content_tree,
                    tags: event.tags.clone(),
                    alt_text,
                })
            }),
        _ => {
            // Unknown / extensibility escape hatch.
            // Native code can further dispatch on `projection.kind` and read raw
            // `tags` / `content_tree` to implement any custom kind without Rust changes.
            let alt_text = tag_value("alt");

            EmbedKindProjection::Unknown(UnknownProjection {
                kind: event.kind,
                author_pubkey,
                created_at,
                content: event.content.clone(),
                content_tree,
                tags: event.tags.clone(),
                alt_text,
            })
        }
    }
}

/// Very small best-effort media URL extractor for the ShortNote preview path.
/// Looks for http(s) URLs that end with common image/video/audio extensions.
/// This is intentionally lightweight; full media classification already exists
/// in the tokenizer pipeline for richer cases.
fn extract_top_level_media(content: &str) -> Vec<String> {
    // Minimal regex-free scan for MVP. A real implementation can reuse
    // the existing URL tokenizer or a shared media classifier.
    content
        .split_whitespace()
        .filter(|w| {
            let lower = w.to_lowercase();
            (lower.starts_with("http://") || lower.starts_with("https://"))
                && (lower.ends_with(".jpg")
                    || lower.ends_with(".jpeg")
                    || lower.ends_with(".png")
                    || lower.ends_with(".gif")
                    || lower.ends_with(".webp")
                    || lower.ends_with(".mp4")
                    || lower.ends_with(".mov")
                    || lower.ends_with(".webm")
                    || lower.ends_with(".mp3")
                    || lower.ends_with(".wav"))
        })
        .map(|s| s.to_string())
        .collect()
}
