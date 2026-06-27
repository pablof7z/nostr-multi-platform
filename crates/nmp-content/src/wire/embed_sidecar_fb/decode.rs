//! Decode half of the `refs.event.envelopes` typed codec — `NEMB` FlatBuffer →
//! resolver [`EmbeddedEventEnvelope`] map. See the module-root doc ([`super`])
//! for the layout / regeneration contract.
//!
//! Honours D6 (no panics): every entry point returns `Err(String)` on malformed
//! input; there are no `unwrap`/`expect`/panicking operations here.

use std::collections::BTreeMap;

use super::generated::nmp::embed as fb;
use crate::embed_projection::{
    ArticleProjection, EmbedKindProjection, EmbeddedEventEnvelope, HighlightProjection,
    ProfileProjection, RenderContextWire, ShortNoteProjection, UnknownProjection,
};
use crate::wire::decode_content_tree;

/// See [`super::decode_ref_event_envelopes`].
pub(super) fn decode_ref_event_envelopes(
    bytes: &[u8],
) -> Result<BTreeMap<String, EmbeddedEventEnvelope>, String> {
    if bytes.len() < 8 || !fb::ref_event_envelopes_buffer_has_identifier(bytes) {
        return Err("missing NEMB file identifier".to_string());
    }
    let root = fb::root_as_ref_event_envelopes(bytes)
        .map_err(|e| format!("not a valid RefEventEnvelopes buffer: {e}"))?;

    let mut entries = BTreeMap::new();
    if let Some(fb_entries) = root.entries() {
        for env in fb_entries.iter() {
            let (primary_id, envelope) = decode_envelope(env)?;
            entries.insert(primary_id, envelope);
        }
    }
    Ok(entries)
}

fn decode_envelope(
    env: fb::EmbeddedEventEnvelope<'_>,
) -> Result<(String, EmbeddedEventEnvelope), String> {
    // `primary_id` is the `(key)` field — a required scalar string accessor
    // returning `&str` directly (not `Option`).
    let primary_id = env.primary_id().to_string();
    let projection = decode_projection(
        env.projection()
            .ok_or("EmbeddedEventEnvelope.projection: missing required table")?,
    )?;
    let envelope = EmbeddedEventEnvelope {
        uri: env.uri().unwrap_or("").to_string(),
        primary_id: primary_id.clone(),
        render_context: RenderContextWire {
            depth: env.depth(),
            max_depth: env.max_depth(),
            visited: Vec::new(),
        },
        projection,
        collapsed: env.collapsed(),
        collapse_reason: opt_field(env.has_collapse_reason(), env.collapse_reason()),
    };
    Ok((primary_id, envelope))
}

fn decode_projection(p: fb::EmbedKindProjection<'_>) -> Result<EmbedKindProjection, String> {
    match p.kind() {
        fb::EmbedProjectionKind::ShortNote => {
            let n = p
                .short_note()
                .ok_or("EmbedKindProjection.short_note: missing payload for ShortNote kind")?;
            Ok(EmbedKindProjection::ShortNote(decode_short_note(n)?))
        }
        fb::EmbedProjectionKind::Article => {
            let a = p
                .article()
                .ok_or("EmbedKindProjection.article: missing payload for Article kind")?;
            Ok(EmbedKindProjection::Article(decode_article(a)?))
        }
        fb::EmbedProjectionKind::Highlight => {
            let h = p
                .highlight()
                .ok_or("EmbedKindProjection.highlight: missing payload for Highlight kind")?;
            Ok(EmbedKindProjection::Highlight(decode_highlight(h)?))
        }
        fb::EmbedProjectionKind::Profile => {
            let pr = p
                .profile()
                .ok_or("EmbedKindProjection.profile: missing payload for Profile kind")?;
            Ok(EmbedKindProjection::Profile(decode_profile(pr)?))
        }
        fb::EmbedProjectionKind::Unknown => {
            let u = p
                .unknown()
                .ok_or("EmbedKindProjection.unknown: missing payload for Unknown kind")?;
            Ok(EmbedKindProjection::Unknown(decode_unknown(u)?))
        }
        other => Err(format!(
            "EmbedKindProjection.kind: unknown discriminant {other:?}"
        )),
    }
}

fn decode_short_note(n: fb::ShortNoteProjection<'_>) -> Result<ShortNoteProjection, String> {
    let content_tree = decode_tree(n.content_tree(), "ShortNoteProjection.content_tree")?;
    Ok(ShortNoteProjection {
        id: str_field(n.id(), "ShortNoteProjection.id")?,
        author_pubkey: str_field(n.author_pubkey(), "ShortNoteProjection.author_pubkey")?,
        author_display_name: opt_field(n.has_author_display_name(), n.author_display_name()),
        author_picture_url: opt_field(n.has_author_picture_url(), n.author_picture_url()),
        created_at: n.created_at(),
        content_tree,
        media_urls: str_vec(n.media_urls()),
    })
}

fn decode_article(a: fb::ArticleProjection<'_>) -> Result<ArticleProjection, String> {
    let content_tree = decode_tree(a.content_tree(), "ArticleProjection.content_tree")?;
    Ok(ArticleProjection {
        id: str_field(a.id(), "ArticleProjection.id")?,
        author_pubkey: str_field(a.author_pubkey(), "ArticleProjection.author_pubkey")?,
        author_display_name: opt_field(a.has_author_display_name(), a.author_display_name()),
        author_picture_url: opt_field(a.has_author_picture_url(), a.author_picture_url()),
        created_at: a.created_at(),
        title: opt_field(a.has_title(), a.title()),
        summary: opt_field(a.has_summary(), a.summary()),
        hero_image_url: opt_field(a.has_hero_image_url(), a.hero_image_url()),
        d_tag: str_field(a.d_tag(), "ArticleProjection.d_tag")?,
        content_tree,
    })
}

fn decode_highlight(h: fb::HighlightProjection<'_>) -> Result<HighlightProjection, String> {
    Ok(HighlightProjection {
        id: str_field(h.id(), "HighlightProjection.id")?,
        author_pubkey: str_field(h.author_pubkey(), "HighlightProjection.author_pubkey")?,
        author_display_name: opt_field(h.has_author_display_name(), h.author_display_name()),
        created_at: h.created_at(),
        highlighted_text: str_field(h.highlighted_text(), "HighlightProjection.highlighted_text")?,
        source_event_id: opt_field(h.has_source_event_id(), h.source_event_id()),
        source_event_addr: opt_field(h.has_source_event_addr(), h.source_event_addr()),
        source_url: opt_field(h.has_source_url(), h.source_url()),
        context: opt_field(h.has_context(), h.context()),
    })
}

fn decode_profile(p: fb::ProfileProjection<'_>) -> Result<ProfileProjection, String> {
    Ok(ProfileProjection {
        pubkey: str_field(p.pubkey(), "ProfileProjection.pubkey")?,
        display_name: opt_field(p.has_display_name(), p.display_name()),
        picture_url: opt_field(p.has_picture_url(), p.picture_url()),
        about: opt_field(p.has_about(), p.about()),
        nip05: opt_field(p.has_nip05(), p.nip05()),
        lud16: opt_field(p.has_lud16(), p.lud16()),
        banner_url: opt_field(p.has_banner_url(), p.banner_url()),
    })
}

fn decode_unknown(u: fb::UnknownProjection<'_>) -> Result<UnknownProjection, String> {
    let content_tree = decode_tree(u.content_tree(), "UnknownProjection.content_tree")?;
    let mut tags = Vec::new();
    if let Some(fb_tags) = u.tags() {
        for row in fb_tags.iter() {
            tags.push(str_vec(row.values()));
        }
    }
    Ok(UnknownProjection {
        kind: u.kind(),
        author_pubkey: str_field(u.author_pubkey(), "UnknownProjection.author_pubkey")?,
        author_display_name: opt_field(u.has_author_display_name(), u.author_display_name()),
        author_picture_url: opt_field(u.has_author_picture_url(), u.author_picture_url()),
        created_at: u.created_at(),
        content: str_field(u.content(), "UnknownProjection.content")?,
        content_tree,
        tags,
        alt_text: opt_field(u.has_alt_text(), u.alt_text()),
    })
}

/// Decode a nested `ContentTreeWire` (`NFCT`) buffer carried as opaque bytes.
fn decode_tree(
    bytes: Option<flatbuffers::Vector<'_, u8>>,
    ctx: &str,
) -> Result<crate::wire::ContentTreeWire, String> {
    let raw = bytes.ok_or_else(|| format!("{ctx}: missing required body buffer"))?;
    decode_content_tree(raw.bytes())
}

/// Collect a FlatBuffers string vector into an owned `Vec<String>` (empty when
/// the field is absent).
fn str_vec(v: Option<flatbuffers::Vector<'_, flatbuffers::ForwardsUOffset<&str>>>) -> Vec<String> {
    v.map(|vec| vec.iter().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Decode a `(has_*, value)` pair back into `Option<String>`: `has == false`
/// yields `None` regardless of the (empty) placeholder string.
fn opt_field(present: bool, value: Option<&str>) -> Option<String> {
    if present {
        Some(value.unwrap_or("").to_string())
    } else {
        None
    }
}

/// Require a present string field; an absent FlatBuffers string on a mandatory
/// slot is a decode error.
fn str_field(value: Option<&str>, ctx: &str) -> Result<String, String> {
    value
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: missing required string field"))
}
