//! Encode half of the `refs.event.envelopes` typed projection codec — the
//! resolved [`EmbeddedEventEnvelope`] map → `NEMB` FlatBuffer. See the
//! module-root doc ([`super`]) for the layout / regeneration contract.

use std::collections::BTreeMap;

use flatbuffers::{FlatBufferBuilder, WIPOffset};

use super::generated::nmp::embed as fb;
use crate::embed_projection::{
    ArticleProjection, EmbedKindProjection, EmbeddedEventEnvelope, HighlightProjection,
    ProfileProjection, ShortNoteProjection, UnknownProjection,
};
use crate::wire::encode_content_tree;

/// See [`super::encode_ref_event_envelopes`].
#[must_use]
pub(super) fn encode_ref_event_envelopes(
    entries: &BTreeMap<String, EmbeddedEventEnvelope>,
) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let entry_offsets: Vec<WIPOffset<fb::EmbeddedEventEnvelope<'_>>> = entries
        .values()
        .map(|env| encode_envelope(&mut fbb, env))
        .collect();
    let entries_vec = fbb.create_vector(&entry_offsets);

    let root = fb::RefEventEnvelopes::create(
        &mut fbb,
        &fb::RefEventEnvelopesArgs {
            entries: Some(entries_vec),
        },
    );
    fb::finish_ref_event_envelopes_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_envelope<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    env: &EmbeddedEventEnvelope,
) -> WIPOffset<fb::EmbeddedEventEnvelope<'a>> {
    let projection = encode_projection(fbb, &env.projection);
    let primary_id = fbb.create_string(&env.primary_id);
    let uri = fbb.create_string(&env.uri);
    let (has_collapse_reason, collapse_reason) = opt_string(fbb, env.collapse_reason.as_deref());

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
    fb::TagRow::create(
        fbb,
        &fb::TagRowArgs {
            values: Some(values),
        },
    )
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
