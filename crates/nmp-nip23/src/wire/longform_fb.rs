//! Typed FlatBuffers wire codec for the NIP-23 long-form snapshot projection
//! ([`crate::LongformProjection`]).
//!
//! This is the **typed sidecar** (ADR-0037) carried in every `SnapshotFrame`'s
//! `typed_projections` slot under the key `nmp.nip23.articles`. Unlike the other
//! typed codecs in this crate, the long-form projection has **no** generic JSON
//! `projections` counterpart: the projection is registered through
//! `AppHost::register_typed_snapshot_projection` only — the JSON map is being
//! retired and this typed payload is the surface hosts read.
//!
//! The shape mirrors the existing NIP-23 resolver output
//! ([`nmp_content::embed_projection::ArticleProjection`]); see
//! `schema/longform.fbs`
//! for the field map. The full article body
//! ([`ArticleProjection::content_tree`]) is carried as the verbatim
//! [`ContentTreeWire`](nmp_content::wire::ContentTreeWire) typed buffer (`NFCT`
//! root) via the existing [`encode_content_tree`](nmp_content::wire::encode_content_tree)
//! codec — reused as an opaque-bytes unit, not re-`include`d into this schema.
//!
//! Honours D6 (no panics): [`decode_longform_articles`] returns `Err(String)` on
//! any malformed input; there are no `unwrap`/`expect`/panicking operations on
//! the decode path.
//!
//! ## Regenerating the bindings
//!
//! The checked-in bindings in `wire/generated/longform_generated.rs` are
//! produced by `flatc` from `schema/longform.fbs`. Regenerate only with the
//! workspace FlatBuffers pin (`25.12.19`), enforced by
//! `ci/check-flatbuffers-version-pins.sh`. The schema is self-contained:
//!
//! ```sh
//! flatc --rust -o crates/nmp-nip23/src/wire/generated \
//!       crates/nmp-nip23/schema/longform.fbs
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
#[path = "generated/longform_generated.rs"]
pub mod generated;

use std::collections::BTreeMap;

use flatbuffers::{FlatBufferBuilder, WIPOffset};
use generated::nmp::nip_23 as fb;

use nmp_content::embed_projection::ArticleProjection;
use nmp_content::wire::{decode_content_tree, encode_content_tree};

use crate::ArticleFeedItem;

/// Stable schema identifier carried in the typed-projection envelope.
pub const SCHEMA_ID: &str = "nmp.nip23.articles";
/// FlatBuffers file identifier embedded in every buffer this module emits.
pub const FILE_IDENTIFIER: &[u8; 4] = b"NL23";
/// Wire schema version. Bump on any breaking change to `longform.fbs`.
/// v2 (#2514): dropped author `display_name`/`picture` from `ArticleDocument` —
/// non-`Profile` tables carry raw `author_pubkey` only; display joins at L5.
pub const SCHEMA_VERSION: u32 = 2;

/// Owned, decoded form of the `nmp.nip23.articles` projection — the round-trip
/// counterpart of the encoder, used by Rust consumers and proof tests.
#[derive(Clone, Debug, PartialEq)]
pub struct LongformArticles {
    /// Feed list, trimmed summaries, newest-first (encoder preserves order).
    pub articles: Vec<ArticleFeedItem>,
    /// Full documents keyed by addressable coordinate `kind:author_hex:d_tag`.
    pub documents: BTreeMap<String, ArticleProjection>,
}

// --- encode ---------------------------------------------------------------

/// Encode the long-form projection (the feed-list rows and the full documents
/// keyed by addressable coordinate) to typed FlatBuffers bytes (with the `NL23`
/// file identifier).
///
/// `articles` order is preserved verbatim. `documents` is encoded in
/// [`BTreeMap`] (ascending-address) order so the `(key)`-keyed `documents`
/// vector is sorted — a host may binary-search it by `address`.
#[must_use]
pub fn encode_longform_articles(
    articles: &[ArticleFeedItem],
    documents: &BTreeMap<String, ArticleProjection>,
) -> Vec<u8> {
    let mut fbb = FlatBufferBuilder::new();

    let article_offsets: Vec<WIPOffset<fb::ArticleFeedItem<'_>>> = articles
        .iter()
        .map(|item| encode_feed_item(&mut fbb, item))
        .collect();
    let articles_vec = fbb.create_vector(&article_offsets);

    let doc_offsets: Vec<WIPOffset<fb::ArticleDocument<'_>>> = documents
        .iter()
        .map(|(address, article)| encode_document(&mut fbb, address, article))
        .collect();
    let documents_vec = fbb.create_vector(&doc_offsets);

    let root = fb::LongformArticles::create(
        &mut fbb,
        &fb::LongformArticlesArgs {
            articles: Some(articles_vec),
            documents: Some(documents_vec),
        },
    );
    fb::finish_longform_articles_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

fn encode_feed_item<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    item: &ArticleFeedItem,
) -> WIPOffset<fb::ArticleFeedItem<'a>> {
    let address = fbb.create_string(&item.address);
    let id = fbb.create_string(&item.id);
    let author_pubkey = fbb.create_string(&item.author_pubkey);
    let title = fbb.create_string(&item.title);
    let summary = fbb.create_string(&item.summary);
    let hero_image_url = fbb.create_string(&item.hero_image_url);
    let d_tag = fbb.create_string(&item.d_tag);

    fb::ArticleFeedItem::create(
        fbb,
        &fb::ArticleFeedItemArgs {
            address: Some(address),
            id: Some(id),
            author_pubkey: Some(author_pubkey),
            title: Some(title),
            summary: Some(summary),
            hero_image_url: Some(hero_image_url),
            d_tag: Some(d_tag),
            created_at: item.created_at,
        },
    )
}

fn encode_document<'a>(
    fbb: &mut FlatBufferBuilder<'a>,
    address: &str,
    article: &ArticleProjection,
) -> WIPOffset<fb::ArticleDocument<'a>> {
    // Encode the article body to the existing `ContentTreeWire` (`NFCT`) buffer,
    // carried verbatim as opaque bytes (no schema `include`).
    let content_tree_bytes = encode_content_tree(&article.content_tree);

    let address = fbb.create_string(address);
    let id = fbb.create_string(&article.id);
    let author_pubkey = fbb.create_string(&article.author_pubkey);
    let d_tag = fbb.create_string(&article.d_tag);
    let content_tree = fbb.create_vector(&content_tree_bytes);

    // `Option<String>` → `has_* : bool` + value string (present-but-empty round
    // -trips distinctly from absent). Absent fields write an empty string with
    // `has_* = false`.
    let (has_title, title) = opt_string(fbb, article.title.as_deref());
    let (has_summary, summary) = opt_string(fbb, article.summary.as_deref());
    let (has_hero_image_url, hero_image_url) = opt_string(fbb, article.hero_image_url.as_deref());

    fb::ArticleDocument::create(
        fbb,
        &fb::ArticleDocumentArgs {
            address: Some(address),
            id: Some(id),
            author_pubkey: Some(author_pubkey),
            created_at: article.created_at,
            has_title,
            title: Some(title),
            has_summary,
            summary: Some(summary),
            has_hero_image_url,
            hero_image_url: Some(hero_image_url),
            d_tag: Some(d_tag),
            content_tree: Some(content_tree),
        },
    )
}

/// Encode an `Option<&str>` as a `(has_*, value)` pair: `Some` → `(true, v)`,
/// `None` → `(false, "")`. The empty placeholder keeps the field non-absent so a
/// decoder never confuses a missing string slot with `None`.
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

/// Decode typed FlatBuffers bytes (as produced by [`encode_longform_articles`])
/// back into a [`LongformArticles`]. Returns an error string on any malformed
/// input or missing required field.
pub fn decode_longform_articles(bytes: &[u8]) -> Result<LongformArticles, String> {
    if bytes.len() < 8 || !fb::longform_articles_buffer_has_identifier(bytes) {
        return Err("missing NL23 file identifier".to_string());
    }
    let root = fb::root_as_longform_articles(bytes)
        .map_err(|e| format!("not a valid LongformArticles buffer: {e}"))?;

    let mut articles = Vec::new();
    if let Some(fb_articles) = root.articles() {
        articles.reserve(fb_articles.len());
        for item in fb_articles.iter() {
            articles.push(decode_feed_item(item)?);
        }
    }

    let mut documents = BTreeMap::new();
    if let Some(fb_documents) = root.documents() {
        for doc in fb_documents.iter() {
            let (address, article) = decode_document(doc)?;
            documents.insert(address, article);
        }
    }

    Ok(LongformArticles {
        articles,
        documents,
    })
}

fn decode_feed_item(item: fb::ArticleFeedItem<'_>) -> Result<ArticleFeedItem, String> {
    Ok(ArticleFeedItem {
        address: str_field(item.address(), "ArticleFeedItem.address")?,
        id: str_field(item.id(), "ArticleFeedItem.id")?,
        author_pubkey: str_field(item.author_pubkey(), "ArticleFeedItem.author_pubkey")?,
        title: str_field(item.title(), "ArticleFeedItem.title")?,
        summary: str_field(item.summary(), "ArticleFeedItem.summary")?,
        hero_image_url: str_field(item.hero_image_url(), "ArticleFeedItem.hero_image_url")?,
        d_tag: str_field(item.d_tag(), "ArticleFeedItem.d_tag")?,
        created_at: item.created_at(),
    })
}

fn decode_document(doc: fb::ArticleDocument<'_>) -> Result<(String, ArticleProjection), String> {
    let address = doc.address().to_string();
    let content_tree_bytes = doc
        .content_tree()
        .ok_or("ArticleDocument.content_tree: missing required body buffer")?;
    let content_tree = decode_content_tree(content_tree_bytes.bytes())?;

    let article = ArticleProjection {
        id: str_field(doc.id(), "ArticleDocument.id")?,
        author_pubkey: str_field(doc.author_pubkey(), "ArticleDocument.author_pubkey")?,
        created_at: doc.created_at(),
        title: opt_field(doc.has_title(), doc.title()),
        summary: opt_field(doc.has_summary(), doc.summary()),
        hero_image_url: opt_field(doc.has_hero_image_url(), doc.hero_image_url()),
        d_tag: str_field(doc.d_tag(), "ArticleDocument.d_tag")?,
        content_tree,
    };
    Ok((address, article))
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
