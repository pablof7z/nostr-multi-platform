//! Typed FlatBuffers wire codec for the `claimed_event_embeds` snapshot sidecar
//! (issue #1283 / ADR-0034 §embed-sidecar).
//!
//! This is the **typed** counterpart of the JSON `claimed_event_embeds`
//! projection (`crates/nmp-ffi/src/embed_sidecar.rs`). Both carry the same
//! pre-resolved `primary_id -> EmbeddedEventEnvelope` map; the typed payload is
//! what a typed-frame shell (Chirp) decodes so it never re-implements the
//! `match kind` resolver in Swift. The JSON projection stays in parallel for the
//! gallery shell. See `schema/embed_sidecar.fbs` for the field map.
//!
//! The shape mirrors the existing resolver types
//! ([`EmbeddedEventEnvelope`](crate::embed_projection::EmbeddedEventEnvelope) /
//! [`EmbedKindProjection`](crate::embed_projection::EmbedKindProjection)) — never
//! a bespoke re-parse. Per-kind `content_tree` bodies are carried as the verbatim
//! [`ContentTreeWire`](crate::wire::ContentTreeWire) typed buffer (`NFCT` root)
//! via the existing [`encode_content_tree`](crate::wire::encode_content_tree)
//! codec, reused as an opaque-bytes unit (no schema `include`), exactly as
//! `longform_fb` carries the article body.
//!
//! Honours D6 (no panics): [`decode_claimed_event_embeds`] returns `Err(String)`
//! on any malformed input; there are no `unwrap`/`expect`/panicking operations on
//! the decode path.
//!
//! ## Regenerating the bindings
//!
//! The checked-in bindings in `wire/generated/embed_sidecar_generated.rs` are
//! produced by `flatc` from `schema/embed_sidecar.fbs`. Regenerate only with the
//! workspace FlatBuffers pin (`25.12.19`), enforced by
//! `ci/check-flatbuffers-version-pins.sh`. The schema is self-contained:
//!
//! ```sh
//! flatc --rust -o crates/nmp-content/src/wire/generated \
//!       crates/nmp-content/schema/embed_sidecar.fbs
//! rustfmt --edition 2021 \
//!       crates/nmp-content/src/wire/generated/embed_sidecar_generated.rs
//! ```

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This single generated module — and only it — opts
// back into `unsafe`. No hand-written code in this file uses `unsafe`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/embed_sidecar_generated.rs"]
pub mod generated;

use std::collections::BTreeMap;

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use generated::nmp::embed as fb;

use crate::embed_projection::{
    ArticleProjection, EmbedKindProjection, EmbeddedEventEnvelope, HighlightProjection,
    ProfileProjection, RenderContextWire, ShortNoteProjection, UnknownProjection,
};
use crate::wire::{decode_content_tree, encode_content_tree};

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "claimed_event_embeds";
/// Snapshot-projection key the typed sidecar is emitted under (matches the JSON
/// projection key so a host's `typed<K> ?? json<k>` fallback lines up).
pub const PROJECTION_KEY: &str = "claimed_event_embeds";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NEMB";
/// Wire schema version. Bump on any breaking change to `embed_sidecar.fbs`.
pub const SCHEMA_VERSION: u32 = 1;

// --- encode ---------------------------------------------------------------

/// Encode the `claimed_event_embeds` projection (envelopes keyed by
/// `primary_id`) to typed FlatBuffers bytes (with the `NEMB` file identifier).
///
/// `entries` is encoded in [`BTreeMap`] (ascending-`primary_id`) order so the
/// `(key)`-keyed `entries` vector is sorted — a host may binary-search it by
/// `primary_id`.
#[must_use]
pub fn encode_claimed_event_embeds(entries: &BTreeMap<String, EmbeddedEventEnvelope>) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let entry_offsets: Vec<WIPOffset<fb::EmbeddedEventEnvelope<'_>>> = entries
        .values()
        .map(|env| encode_envelope(&mut fbb, env))
        .collect();
    let entries_vec = fbb.create_vector(&entry_offsets);

    let root = fb::ClaimedEventEmbeds::create(
        &mut fbb,
        &fb::ClaimedEventEmbedsArgs {
            entries: Some(entries_vec),
        },
    );
    fb::finish_claimed_event_embeds_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_envelope<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    env: &EmbeddedEventEnvelope,
) -> WIPOffset<fb::EmbeddedEventEnvelope<'a>> {
    let projection = encode_projection(fbb, &env.projection);
    let primary_id = fbb.create_string(&env.primary_id);
    let uri = fbb.create_string(&env.uri);
    let (has_collapse_reason, collapse_reason) =
        opt_string(fbb, env.collapse_reason.as_deref());

    let mut builder = fb::EmbeddedEventEnvelopeBuilder::new(fbb);
    builder.add_primary_id(primary_id);
    builder.add_uri(uri);
    builder.add_depth(env.render_context.depth);
    builder.add_max_depth(env.render_context.max_depth);
    builder.add_collapsed(env.collapsed);
    builder.add_has_collapse_reason(has_collapse_reason);
    builder.add_collapse_reason(collapse_reason);
    builder.add_projection(projection);
    builder.finish()
}

fn encode_projection<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    projection: &EmbedKindProjection,
) -> WIPOffset<fb::EmbedKindProjection<'a>> {
    match projection {
        EmbedKindProjection::ShortNote(p) => {
            let payload = encode_short_note(fbb, p);
            let mut b = fb::EmbedKindProjectionBuilder::new(fbb);
            b.add_kind(fb::EmbedProjectionKind::ShortNote);
            b.add_short_note(payload);
            b.finish()
        }
        EmbedKindProjection::Article(p) => {
            let payload = encode_article(fbb, p);
            let mut b = fb::EmbedKindProjectionBuilder::new(fbb);
            b.add_kind(fb::EmbedProjectionKind::Article);
            b.add_article(payload);
            b.finish()
        }
        EmbedKindProjection::Highlight(p) => {
            let payload = encode_highlight(fbb, p);
            let mut b = fb::EmbedKindProjectionBuilder::new(fbb);
            b.add_kind(fb::EmbedProjectionKind::Highlight);
            b.add_highlight(payload);
            b.finish()
        }
        EmbedKindProjection::Profile(p) => {
            let payload = encode_profile(fbb, p);
            let mut b = fb::EmbedKindProjectionBuilder::new(fbb);
            b.add_kind(fb::EmbedProjectionKind::Profile);
            b.add_profile(payload);
            b.finish()
        }
        EmbedKindProjection::Unknown(p) => {
            let payload = encode_unknown(fbb, p);
            let mut b = fb::EmbedKindProjectionBuilder::new(fbb);
            b.add_kind(fb::EmbedProjectionKind::Unknown);
            b.add_unknown(payload);
            b.finish()
        }
    }
}

fn encode_short_note<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    p: &ShortNoteProjection,
) -> WIPOffset<fb::ShortNoteProjection<'a>> {
    let content_tree_bytes = encode_content_tree(&p.content_tree);
    let id = fbb.create_string(&p.id);
    let author_pubkey = fbb.create_string(&p.author_pubkey);
    let (has_adn, adn) = opt_string(fbb, p.author_display_name.as_deref());
    let (has_apu, apu) = opt_string(fbb, p.author_picture_url.as_deref());
    let content_tree = fbb.create_vector(&content_tree_bytes);
    let media_strs: Vec<WIPOffset<&str>> =
        p.media_urls.iter().map(|u| fbb.create_string(u)).collect();
    let media_urls = fbb.create_vector(&media_strs);

    let mut b = fb::ShortNoteProjectionBuilder::new(fbb);
    b.add_id(id);
    b.add_author_pubkey(author_pubkey);
    b.add_has_author_display_name(has_adn);
    b.add_author_display_name(adn);
    b.add_has_author_picture_url(has_apu);
    b.add_author_picture_url(apu);
    b.add_created_at(p.created_at);
    b.add_content_tree(content_tree);
    b.add_media_urls(media_urls);
    b.finish()
}

fn encode_article<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    p: &ArticleProjection,
) -> WIPOffset<fb::ArticleProjection<'a>> {
    let content_tree_bytes = encode_content_tree(&p.content_tree);
    let id = fbb.create_string(&p.id);
    let author_pubkey = fbb.create_string(&p.author_pubkey);
    let (has_adn, adn) = opt_string(fbb, p.author_display_name.as_deref());
    let (has_apu, apu) = opt_string(fbb, p.author_picture_url.as_deref());
    let (has_title, title) = opt_string(fbb, p.title.as_deref());
    let (has_summary, summary) = opt_string(fbb, p.summary.as_deref());
    let (has_hero, hero) = opt_string(fbb, p.hero_image_url.as_deref());
    let d_tag = fbb.create_string(&p.d_tag);
    let content_tree = fbb.create_vector(&content_tree_bytes);

    let mut b = fb::ArticleProjectionBuilder::new(fbb);
    b.add_id(id);
    b.add_author_pubkey(author_pubkey);
    b.add_has_author_display_name(has_adn);
    b.add_author_display_name(adn);
    b.add_has_author_picture_url(has_apu);
    b.add_author_picture_url(apu);
    b.add_created_at(p.created_at);
    b.add_has_title(has_title);
    b.add_title(title);
    b.add_has_summary(has_summary);
    b.add_summary(summary);
    b.add_has_hero_image_url(has_hero);
    b.add_hero_image_url(hero);
    b.add_d_tag(d_tag);
    b.add_content_tree(content_tree);
    b.finish()
}

fn encode_highlight<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    p: &HighlightProjection,
) -> WIPOffset<fb::HighlightProjection<'a>> {
    let id = fbb.create_string(&p.id);
    let author_pubkey = fbb.create_string(&p.author_pubkey);
    let (has_adn, adn) = opt_string(fbb, p.author_display_name.as_deref());
    let highlighted_text = fbb.create_string(&p.highlighted_text);
    let (has_sei, sei) = opt_string(fbb, p.source_event_id.as_deref());
    let (has_sea, sea) = opt_string(fbb, p.source_event_addr.as_deref());
    let (has_su, su) = opt_string(fbb, p.source_url.as_deref());
    let (has_ctx, ctx) = opt_string(fbb, p.context.as_deref());

    let mut b = fb::HighlightProjectionBuilder::new(fbb);
    b.add_id(id);
    b.add_author_pubkey(author_pubkey);
    b.add_has_author_display_name(has_adn);
    b.add_author_display_name(adn);
    b.add_created_at(p.created_at);
    b.add_highlighted_text(highlighted_text);
    b.add_has_source_event_id(has_sei);
    b.add_source_event_id(sei);
    b.add_has_source_event_addr(has_sea);
    b.add_source_event_addr(sea);
    b.add_has_source_url(has_su);
    b.add_source_url(su);
    b.add_has_context(has_ctx);
    b.add_context(ctx);
    b.finish()
}

fn encode_profile<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    p: &ProfileProjection,
) -> WIPOffset<fb::ProfileProjection<'a>> {
    let pubkey = fbb.create_string(&p.pubkey);
    let (has_dn, dn) = opt_string(fbb, p.display_name.as_deref());
    let (has_pu, pu) = opt_string(fbb, p.picture_url.as_deref());
    let (has_about, about) = opt_string(fbb, p.about.as_deref());
    let (has_nip05, nip05) = opt_string(fbb, p.nip05.as_deref());
    let (has_lud16, lud16) = opt_string(fbb, p.lud16.as_deref());
    let (has_banner, banner) = opt_string(fbb, p.banner_url.as_deref());

    let mut b = fb::ProfileProjectionBuilder::new(fbb);
    b.add_pubkey(pubkey);
    b.add_has_display_name(has_dn);
    b.add_display_name(dn);
    b.add_has_picture_url(has_pu);
    b.add_picture_url(pu);
    b.add_has_about(has_about);
    b.add_about(about);
    b.add_has_nip05(has_nip05);
    b.add_nip05(nip05);
    b.add_has_lud16(has_lud16);
    b.add_lud16(lud16);
    b.add_has_banner_url(has_banner);
    b.add_banner_url(banner);
    b.finish()
}

fn encode_unknown<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    p: &UnknownProjection,
) -> WIPOffset<fb::UnknownProjection<'a>> {
    let content_tree_bytes = encode_content_tree(&p.content_tree);
    let author_pubkey = fbb.create_string(&p.author_pubkey);
    let (has_adn, adn) = opt_string(fbb, p.author_display_name.as_deref());
    let (has_apu, apu) = opt_string(fbb, p.author_picture_url.as_deref());
    let content = fbb.create_string(&p.content);
    let content_tree = fbb.create_vector(&content_tree_bytes);
    let tag_offsets: Vec<WIPOffset<fb::TagRow<'_>>> =
        p.tags.iter().map(|row| encode_tag_row(fbb, row)).collect();
    let tags = fbb.create_vector(&tag_offsets);
    let (has_alt, alt) = opt_string(fbb, p.alt_text.as_deref());

    let mut b = fb::UnknownProjectionBuilder::new(fbb);
    b.add_kind(p.kind);
    b.add_author_pubkey(author_pubkey);
    b.add_has_author_display_name(has_adn);
    b.add_author_display_name(adn);
    b.add_has_author_picture_url(has_apu);
    b.add_author_picture_url(apu);
    b.add_created_at(p.created_at);
    b.add_content(content);
    b.add_content_tree(content_tree);
    b.add_tags(tags);
    b.add_has_alt_text(has_alt);
    b.add_alt_text(alt);
    b.finish()
}

fn encode_tag_row<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    row: &[String],
) -> WIPOffset<fb::TagRow<'a>> {
    let value_strs: Vec<WIPOffset<&str>> = row.iter().map(|v| fbb.create_string(v)).collect();
    let values = fbb.create_vector(&value_strs);
    fb::TagRow::create(fbb, &fb::TagRowArgs { values: Some(values) })
}

/// Encode an `Option<&str>` as a `(has_*, value)` pair: `Some` → `(true, v)`,
/// `None` → `(false, "")`. Matches the `longform_fb` convention.
fn opt_string<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    value: Option<&str>,
) -> (bool, WIPOffset<&'a str>) {
    match value {
        Some(v) => (true, fbb.create_string(v)),
        None => (false, fbb.create_string("")),
    }
}

// --- decode ---------------------------------------------------------------

/// Decode typed FlatBuffers bytes (as produced by [`encode_claimed_event_embeds`])
/// back into a `primary_id -> EmbeddedEventEnvelope` map. Returns an error string
/// on any malformed input or missing required field.
pub fn decode_claimed_event_embeds(
    bytes: &[u8],
) -> Result<BTreeMap<String, EmbeddedEventEnvelope>, String> {
    if bytes.len() < 8 || !fb::claimed_event_embeds_buffer_has_identifier(bytes) {
        return Err("missing NEMB file identifier".to_string());
    }
    let root = fb::root_as_claimed_event_embeds(bytes)
        .map_err(|e| format!("not a valid ClaimedEventEmbeds buffer: {e}"))?;

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
        other => Err(format!("EmbedKindProjection.kind: unknown discriminant {other:?}")),
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

#[cfg(test)]
mod tests;
