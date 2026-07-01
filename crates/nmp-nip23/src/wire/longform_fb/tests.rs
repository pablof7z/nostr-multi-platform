//! Round-trip + D6 proof tests for the NL23 long-form typed codec.

use std::collections::BTreeMap;

use super::{
    decode_longform_articles, encode_longform_articles, LongformArticles, FILE_IDENTIFIER,
    SCHEMA_ID, SCHEMA_VERSION,
};
use nmp_content::embed_projection::ArticleProjection;
use nmp_content::wire::ContentTreeWire;
use nmp_content::{tokenize_with_kind, RenderMode};

use crate::ArticleFeedItem;

fn body_tree(markdown: &str) -> ContentTreeWire {
    tokenize_with_kind(markdown, &[], RenderMode::Auto, 30_023).to_wire()
}

fn feed_item(address: &str, created_at: u64) -> ArticleFeedItem {
    ArticleFeedItem {
        address: address.to_string(),
        id: "a".repeat(64),
        author_pubkey: "b".repeat(64),
        title: "Title".to_string(),
        summary: "Summary".to_string(),
        hero_image_url: "https://img/hero.png".to_string(),
        d_tag: "slug".to_string(),
        created_at,
    }
}

fn document(d_tag: &str, title: Option<&str>) -> ArticleProjection {
    ArticleProjection {
        id: "c".repeat(64),
        author_pubkey: "d".repeat(64),
        created_at: 4_242,
        title: title.map(str::to_string),
        summary: Some("a summary".to_string()),
        hero_image_url: None,
        d_tag: d_tag.to_string(),
        content_tree: body_tree("# Heading\n\nA *paragraph* with text."),
    }
}

#[test]
fn schema_identity_is_stable() {
    assert_eq!(SCHEMA_ID, "nmp.nip23.articles");
    assert_eq!(FILE_IDENTIFIER, b"NL23");
    assert_eq!(SCHEMA_VERSION, 2);
}

#[test]
fn empty_round_trips() {
    let bytes = encode_longform_articles(&[], &BTreeMap::new());
    let decoded = decode_longform_articles(&bytes).expect("empty decodes");
    assert_eq!(
        decoded,
        LongformArticles {
            articles: Vec::new(),
            documents: BTreeMap::new(),
        }
    );
}

#[test]
fn full_round_trip_preserves_articles_documents_and_body() {
    let articles = vec![
        feed_item("30023:b:slug", 2_000),
        feed_item("30023:c:other", 1_000),
    ];
    let mut documents = BTreeMap::new();
    documents.insert(
        "30023:b:slug".to_string(),
        document("slug", Some("Present Title")),
    );
    // A document whose `title` tag is absent — `None` must survive the
    // `has_title` presence flag distinctly from a present empty string.
    documents.insert("30023:c:other".to_string(), document("other", None));

    let bytes = encode_longform_articles(&articles, &documents);
    let decoded = decode_longform_articles(&bytes).expect("round trips");

    // Articles preserved verbatim, in order.
    assert_eq!(decoded.articles, articles);

    // Documents preserved, keyed by address.
    assert_eq!(decoded.documents.len(), 2);
    let with_title = &decoded.documents["30023:b:slug"];
    assert_eq!(with_title.title.as_deref(), Some("Present Title"));
    assert_eq!(with_title.author_pubkey, "d".repeat(64));
    // `None` title survives as absent (not "").
    assert_eq!(with_title.hero_image_url, None);

    let no_title = &decoded.documents["30023:c:other"];
    assert_eq!(
        no_title.title, None,
        "absent title round-trips as None, not empty string"
    );

    // The article body round-trips through the nested NFCT buffer.
    assert_eq!(
        with_title.content_tree,
        document("slug", Some("Present Title")).content_tree
    );
}

#[test]
fn present_empty_string_round_trips_distinctly_from_absent() {
    let mut documents = BTreeMap::new();
    let mut doc = document("slug", Some(String::new().as_str()));
    doc.summary = Some(String::new());
    documents.insert("30023:b:slug".to_string(), doc);

    let bytes = encode_longform_articles(&[], &documents);
    let decoded = decode_longform_articles(&bytes).expect("round trips");
    let d = &decoded.documents["30023:b:slug"];
    // present-but-empty stays `Some("")`, NOT collapsed to `None`.
    assert_eq!(d.title, Some(String::new()));
    assert_eq!(d.summary, Some(String::new()));
}

#[test]
fn garbage_bytes_do_not_panic() {
    assert!(decode_longform_articles(&[]).is_err());
    assert!(decode_longform_articles(&[0u8; 4]).is_err());
    assert!(decode_longform_articles(b"NOTAVALIDBUFFERATALL").is_err());
}

/// The `documents` vector is `(key)`-sorted by `address`, so a host can
/// binary-search it with the generated `lookup_by_key`. Encoding from a
/// `BTreeMap` (ascending-address) satisfies the sorted invariant; this proves
/// the host-side lookup actually resolves a document by its address.
#[test]
fn documents_are_key_sorted_and_lookup_by_key_resolves() {
    use super::generated::nmp::nip_23 as fb;

    let mut documents = BTreeMap::new();
    documents.insert("30023:b:slug".to_string(), document("slug", Some("Beta")));
    documents.insert(
        "30023:a:first".to_string(),
        document("first", Some("Alpha")),
    );
    documents.insert("30023:c:last".to_string(), document("last", Some("Gamma")));

    let bytes = encode_longform_articles(&[], &documents);
    let root = fb::root_as_longform_articles(&bytes).expect("valid buffer");
    let docs = root.documents().expect("documents vector present");

    let hit = docs
        .lookup_by_key("30023:a:first", |doc, key| doc.key_compare_with_value(key))
        .expect("address present in the sorted key-vector");
    assert_eq!(hit.address(), "30023:a:first");
    assert_eq!(hit.title(), Some("Alpha"));

    // A missing address resolves to None (not a panic, not a wrong row).
    assert!(docs
        .lookup_by_key("30023:z:absent", |doc, key| doc.key_compare_with_value(key))
        .is_none());
}
