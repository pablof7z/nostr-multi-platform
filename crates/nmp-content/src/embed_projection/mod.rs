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
use crate::longform::KIND_LONG_FORM_ARTICLE;
use crate::mode::RenderMode;
use crate::tokenize_with_kind;
use crate::wire::ContentTreeWire;

// Kinds this rendering sidecar dispatches on. Named so the `match event.kind`
// arms below read as protocol concepts rather than bare numeric literals.
// `KIND_LONG_FORM_ARTICLE` is re-used from [`crate::longform`].
//
// TODO(#1493): migrate these kind constants to `nmp-kinds` once that crate
// owns the shared kind registry (another lane owns `nmp-kinds`; do not edit it
// from here).
/// NIP-01 profile-metadata kind (`0`).
const KIND_PROFILE_METADATA: u32 = 0;
/// NIP-01 short text note kind (`1`).
const KIND_SHORT_NOTE: u32 = 1;
/// NIP-84 highlight kind (`9802`).
const KIND_HIGHLIGHT: u32 = 9_802;

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
            // Profile (kind:0). The embed IS a kind:0 event, so its `content` is
            // the profile metadata JSON — parse it here (a rendering/projection
            // concern, D0-clean: serde_json on already-claimed content, no crypto,
            // no kind dispatch leaking to the shell). This is what lets the shell
            // delete its in-Swift `parseProfileMetadata` (#1283) and fixes the
            // #1299 inverted `display_name` precedence: NIP-01/24 says
            // `display_name` wins over `displayName` wins over `name` (mirrors the
            // kernel's `parse_profile` in `nmp-core::kernel::nostr`).
            EmbedKindProjection::Profile(parse_profile_metadata(&event.content, author_pubkey))
        }
        KIND_SHORT_NOTE => {
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
        KIND_HIGHLIGHT => {
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
        KIND_LONG_FORM_ARTICLE => {
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

/// Parse a kind:0 profile metadata JSON `content` into a [`ProfileProjection`].
///
/// NIP-01/24 display-name precedence: `display_name` wins over the camelCase
/// `displayName` alias wins over `name` (mirrors `nmp_core::kernel::nostr::
/// parse_profile`; the old in-Swift resolver had this INVERTED — see #1299).
/// Empty / whitespace-only values are normalised to `None` so the shell never
/// renders a blank name. `picture` / `banner` must be http(s) URLs. A malformed
/// or empty content yields a projection with only the `pubkey` populated (D6 —
/// never a panic).
fn parse_profile_metadata(content: &str, pubkey: String) -> ProfileProjection {
    #[derive(Default, serde::Deserialize)]
    struct ProfileContent {
        name: Option<String>,
        display_name: Option<String>,
        #[serde(rename = "displayName")]
        display_name_camel: Option<String>,
        picture: Option<String>,
        nip05: Option<String>,
        about: Option<String>,
        lud16: Option<String>,
        banner: Option<String>,
    }

    let parsed = serde_json::from_str::<ProfileContent>(content).unwrap_or_default();
    let non_empty = |value: Option<String>| -> Option<String> {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let http_url = |value: Option<String>| -> Option<String> {
        value.filter(|v| v.starts_with("http://") || v.starts_with("https://"))
    };

    ProfileProjection {
        pubkey,
        // NIP-01/24 precedence: display_name → displayName → name (#1299).
        display_name: non_empty(
            parsed
                .display_name
                .or(parsed.display_name_camel)
                .or(parsed.name),
        ),
        picture_url: http_url(parsed.picture),
        about: non_empty(parsed.about),
        nip05: non_empty(parsed.nip05),
        lud16: non_empty(parsed.lud16),
        banner_url: http_url(parsed.banner),
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
