//! Kind-dispatched embed projection (F-CR-01).
//!
//! This module is the single place in the workspace that performs the
//! `match event.kind` decision for content rendering of embedded events.
//! It produces typed `EmbedKindProjection` variants that native registries
//! consume via `EmbeddedEventEnvelope` on the wire.
//!
//! D0-clean: lives in nmp-content (a rendering sidecar), not nmp-core substrate.
//! See ADR-0034 and docs/plan/m16-kind-dispatch.md for the full contract.

mod envelope;
mod variants;

#[cfg(test)]
mod tests;

pub use envelope::{EmbeddedEventEnvelope, RenderContextWire};
pub use variants::{
    ArticleProjection, EmbedKindProjection, HighlightProjection, ProfileProjection,
    ShortNoteProjection, UnknownProjection,
};

use nmp_core::substrate::KernelEvent;

use crate::context::RenderContext;
use crate::mode::RenderMode;
use crate::tokenize_with_kind;
use crate::wire::ContentTreeWire;

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
        0 => {
            // Profile (kind:0). Content is JSON; for the projection we surface
            // the raw content + a parsed tree. Rich profile fields (name, about,
            // nip05, etc.) are typically enriched by the caller from a live
            // kind:0 projection cache. We keep the resolver minimal.
            EmbedKindProjection::Profile(ProfileProjection {
                pubkey: author_pubkey,
                display_name: None,
                picture_url: None,
                about: None,
                nip05: None,
                lud16: None,
                banner_url: None,
            })
        }
        1 => {
            // Short note
            // Media extraction is a best-effort helper (URLs that look like media).
            // For a more complete implementation this can delegate to an existing
            // media classifier in the crate.
            let media_urls: Vec<String> = extract_top_level_media(&event.content);

            EmbedKindProjection::ShortNote(ShortNoteProjection {
                id,
                author_pubkey,
                author_display_name: None,
                author_picture_url: None,
                created_at,
                content_tree,
                media_urls,
            })
        }
        9802 => {
            // NIP-84 highlight
            let source_event_id = tag_value("e");
            let source_event_addr = tag_value("a");
            let source_url = tag_value("r");
            let context = tag_value("context");

            EmbedKindProjection::Highlight(HighlightProjection {
                id,
                author_pubkey,
                author_display_name: None,
                created_at,
                highlighted_text: event.content.clone(),
                source_event_id,
                source_event_addr,
                source_url,
                context,
            })
        }
        30023 => {
            // Long-form article (NIP-23)
            let title = tag_value("title");
            let summary = tag_value("summary");
            let hero_image_url = tag_value("image");
            let d_tag = tag_value("d").unwrap_or_default();

            EmbedKindProjection::Article(ArticleProjection {
                id,
                author_pubkey,
                author_display_name: None,
                author_picture_url: None,
                created_at,
                title,
                summary,
                hero_image_url,
                d_tag,
                content_tree,
            })
        }
        _ => {
            // Unknown / extensibility escape hatch.
            // Native code can further dispatch on `projection.kind` and read raw
            // `tags` / `content_tree` to implement any custom kind without Rust changes.
            let alt_text = tag_value("alt");

            EmbedKindProjection::Unknown(UnknownProjection {
                kind: event.kind,
                author_pubkey,
                author_display_name: None,
                author_picture_url: None,
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
