//! Round-trip proofs for the `refs.event.envelopes` typed FlatBuffers codec.
//!
//! Every projection variant except owner-registered NIP-23 articles is resolved
//! through the REAL [`resolve_embed_projection`] resolver, wrapped in the
//! FFI-sidecar envelope shape, encoded, and decoded — proving the typed wire
//! preserves the exact resolver output a typed-frame shell consumes.

use std::collections::BTreeMap;

use nmp_core::substrate::KernelEvent;

use super::{decode_ref_event_envelopes, encode_ref_event_envelopes, FILE_IDENTIFIER};
use crate::context::RenderContext;
use crate::embed_projection::{
    resolve_embed_projection, ArticleProjection, EmbedKindProjection, EmbeddedEventEnvelope,
    RenderContextWire,
};
use crate::{tokenize_with_kind, RenderMode};

fn kernel_event(
    id: &str,
    author: &str,
    kind: u32,
    content: &str,
    tags: Vec<Vec<String>>,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at: 1_710_000_000,
        tags,
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

/// Build the FFI-sidecar envelope shape (uri="", depth=0, max_depth=4,
/// collapsed=false) around a resolved projection — mirrors the producer.
fn envelope(primary_id: &str, projection: EmbedKindProjection) -> EmbeddedEventEnvelope {
    EmbeddedEventEnvelope {
        uri: String::new(),
        primary_id: primary_id.to_string(),
        render_context: RenderContextWire {
            depth: 0,
            max_depth: 4,
            visited: Vec::new(),
        },
        projection,
        collapsed: false,
        collapse_reason: None,
    }
}

fn resolve(event: &KernelEvent) -> EmbedKindProjection {
    resolve_embed_projection(event, &RenderContext::new())
}

#[test]
fn empty_map_round_trips_to_empty_map() {
    let entries: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("empty buffer must decode");
    assert!(decoded.is_empty(), "empty input must yield empty map");
}

#[test]
fn buffer_carries_nemb_identifier() {
    let entries: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
    let bytes = encode_ref_event_envelopes(&entries);
    assert_eq!(
        &bytes[4..8],
        FILE_IDENTIFIER,
        "buffer must embed the NEMB id"
    );
}

#[test]
fn short_note_round_trips() {
    let ev = kernel_event(
        "aa".repeat(32).as_str(),
        &"bb".repeat(32),
        1,
        "hello https://x.com/a.png",
        vec![],
    );
    let mut entries = BTreeMap::new();
    entries.insert("p1".to_string(), envelope("p1", resolve(&ev)));

    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("decode");

    let env = decoded.get("p1").expect("p1 present");
    assert_eq!(env.primary_id, "p1");
    assert_eq!(env.render_context.max_depth, 4);
    match &env.projection {
        EmbedKindProjection::ShortNote(n) => {
            assert_eq!(n.author_pubkey, "bb".repeat(32));
            assert_eq!(n.created_at, 1_710_000_000);
        }
        other => panic!("expected ShortNote, got {other:?}"),
    }
}

#[test]
fn article_round_trips_with_optional_tags() {
    let projection = EmbedKindProjection::Article(ArticleProjection {
        id: "art".repeat(16),
        author_pubkey: "cc".repeat(32),
        created_at: 1_710_000_000,
        title: Some("My Title".to_string()),
        summary: Some("A summary".to_string()),
        hero_image_url: None,
        d_tag: "my-article".to_string(),
        content_tree: tokenize_with_kind("# Body", &[], RenderMode::Auto, 30023).to_wire(),
    });
    let mut entries = BTreeMap::new();
    entries.insert("art1".to_string(), envelope("art1", projection));

    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("decode");

    match &decoded.get("art1").unwrap().projection {
        EmbedKindProjection::Article(a) => {
            assert_eq!(a.title.as_deref(), Some("My Title"));
            assert_eq!(a.summary.as_deref(), Some("A summary"));
            assert_eq!(a.hero_image_url, None, "absent image tag must be None");
            assert_eq!(a.d_tag, "my-article");
        }
        other => panic!("expected Article, got {other:?}"),
    }
}

#[test]
fn highlight_round_trips() {
    let tags = vec![vec!["e".to_string(), "source-id".to_string()]];
    let ev = kernel_event(
        "hl".repeat(16).as_str(),
        &"dd".repeat(32),
        9802,
        "quoted",
        tags,
    );
    let mut entries = BTreeMap::new();
    entries.insert("hl1".to_string(), envelope("hl1", resolve(&ev)));

    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("decode");

    match &decoded.get("hl1").unwrap().projection {
        EmbedKindProjection::Highlight(h) => {
            assert_eq!(h.highlighted_text, "quoted");
            assert_eq!(h.source_event_id.as_deref(), Some("source-id"));
            assert_eq!(h.source_url, None);
        }
        other => panic!("expected Highlight, got {other:?}"),
    }
}

#[test]
fn profile_round_trips() {
    // The resolver parses the kind:0 content into the Profile projection
    // (#1283 / #1299). The codec must preserve those parsed fields verbatim.
    let ev = kernel_event(
        "ee".repeat(32).as_str(),
        &"ee".repeat(32),
        0,
        r#"{"name":"Alice","picture":"https://x.com/a.jpg","nip05":"a@x.com"}"#,
        vec![],
    );
    let mut entries = BTreeMap::new();
    entries.insert("ee".repeat(32), envelope(&"ee".repeat(32), resolve(&ev)));

    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("decode");

    match &decoded.get(&"ee".repeat(32)).unwrap().projection {
        EmbedKindProjection::Profile(p) => {
            assert_eq!(p.pubkey, "ee".repeat(32));
            assert_eq!(p.display_name.as_deref(), Some("Alice"));
            assert_eq!(p.picture_url.as_deref(), Some("https://x.com/a.jpg"));
            assert_eq!(p.nip05.as_deref(), Some("a@x.com"));
            assert_eq!(p.about, None, "absent about must stay None");
        }
        other => panic!("expected Profile, got {other:?}"),
    }
}

/// A profile WITH enriched metadata (as a caller would produce after kind:0
/// lookup) must round-trip every `Some(...)` field — proving the `has_*`
/// presence convention preserves enrichment, not just the bare resolver output.
#[test]
fn profile_with_enrichment_round_trips() {
    use crate::embed_projection::ProfileProjection;
    let proj = EmbedKindProjection::Profile(ProfileProjection {
        pubkey: "ab".repeat(32),
        display_name: Some("Alice".to_string()),
        picture_url: Some("https://x.com/a.jpg".to_string()),
        about: None,
        nip05: Some("a@x.com".to_string()),
        lud16: None,
        banner_url: Some("https://x.com/b.jpg".to_string()),
    });
    let mut entries = BTreeMap::new();
    entries.insert("k".to_string(), envelope("k", proj));

    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("decode");

    match &decoded.get("k").unwrap().projection {
        EmbedKindProjection::Profile(p) => {
            assert_eq!(p.display_name.as_deref(), Some("Alice"));
            assert_eq!(p.picture_url.as_deref(), Some("https://x.com/a.jpg"));
            assert_eq!(p.about, None, "absent about must stay None, not Some(\"\")");
            assert_eq!(p.nip05.as_deref(), Some("a@x.com"));
            assert_eq!(p.lud16, None);
            assert_eq!(p.banner_url.as_deref(), Some("https://x.com/b.jpg"));
        }
        other => panic!("expected Profile, got {other:?}"),
    }
}

#[test]
fn unknown_round_trips_with_tags() {
    let tags = vec![
        vec!["alt".to_string(), "an ad".to_string()],
        vec!["price".to_string(), "100".to_string(), "USD".to_string()],
    ];
    let ev = kernel_event(
        "un".repeat(16).as_str(),
        &"ff".repeat(32),
        30402,
        "classified",
        tags,
    );
    let mut entries = BTreeMap::new();
    entries.insert("un1".to_string(), envelope("un1", resolve(&ev)));

    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("decode");

    match &decoded.get("un1").unwrap().projection {
        EmbedKindProjection::Unknown(u) => {
            assert_eq!(u.kind, 30402);
            assert_eq!(u.content, "classified");
            assert_eq!(u.alt_text.as_deref(), Some("an ad"));
            assert_eq!(u.tags.len(), 2);
            assert_eq!(u.tags[1], vec!["price", "100", "USD"]);
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}

#[test]
fn multiple_entries_preserve_keys() {
    let n = kernel_event(
        "a1".repeat(16).as_str(),
        &"11".repeat(32),
        1,
        "note",
        vec![],
    );
    let p = kernel_event(
        "22".repeat(32).as_str(),
        &"22".repeat(32),
        0,
        r#"{"name":"Bob"}"#,
        vec![],
    );
    let mut entries = BTreeMap::new();
    entries.insert("note-key".to_string(), envelope("note-key", resolve(&n)));
    entries.insert("prof-key".to_string(), envelope("prof-key", resolve(&p)));

    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("decode");

    assert_eq!(decoded.len(), 2);
    assert!(matches!(
        decoded.get("note-key").unwrap().projection,
        EmbedKindProjection::ShortNote(_)
    ));
    assert!(matches!(
        decoded.get("prof-key").unwrap().projection,
        EmbedKindProjection::Profile(_)
    ));
}

#[test]
fn garbage_bytes_decode_to_err_not_panic() {
    assert!(decode_ref_event_envelopes(&[]).is_err());
    assert!(decode_ref_event_envelopes(&[0u8; 4]).is_err());
    assert!(decode_ref_event_envelopes(b"XXXXabcd").is_err());
}

// ── Wire-stability regression (#3016) ───────────────────────────────────────
//
// #2514 removed `has_author_display_name`/`author_display_name`/
// `has_author_picture_url`/`author_picture_url` OUTRIGHT from the middle of
// ShortNote/Article/Highlight/Unknown, which reflows every later field's
// FlatBuffers vtable offset — a non-additive schema change. That is exactly
// what made `ArticleProjection.title` land on the byte range `hero_image_url`
// now occupies for any decoder still expecting the pre-#2514 layout: #3016's
// "article.title contains the image URL" and "ShortNote createdAt/content
// never populate" bugs. The fix keeps those four fields as `(deprecated)`
// placeholders at their ORIGINAL position instead of deleting them, so every
// field declared after them keeps a STABLE vtable offset. These tests pin
// that offset contract so nobody can silently reopen the regression by
// deleting a "deprecated" field or reordering a table again.
mod wire_stability {
    use super::super::generated::nmp::embed::{
        ArticleProjection, HighlightProjection, ShortNoteProjection, UnknownProjection,
    };

    #[test]
    fn short_note_projection_offsets_are_stable() {
        assert_eq!(ShortNoteProjection::VT_ID, 4);
        assert_eq!(ShortNoteProjection::VT_AUTHOR_PUBKEY, 6);
        // Slots 8/10/12/14 are the deprecated author-display placeholders.
        assert_eq!(ShortNoteProjection::VT_CREATED_AT, 16);
        assert_eq!(ShortNoteProjection::VT_CONTENT_TREE, 18);
        assert_eq!(ShortNoteProjection::VT_MEDIA_URLS, 20);
    }

    #[test]
    fn article_projection_offsets_are_stable() {
        assert_eq!(ArticleProjection::VT_ID, 4);
        assert_eq!(ArticleProjection::VT_AUTHOR_PUBKEY, 6);
        // Slots 8/10/12/14 are the deprecated author-display placeholders.
        assert_eq!(ArticleProjection::VT_CREATED_AT, 16);
        assert_eq!(ArticleProjection::VT_HAS_TITLE, 18);
        assert_eq!(
            ArticleProjection::VT_TITLE,
            20,
            "title must NOT land on hero_image_url's old offset (#3016)"
        );
        assert_eq!(ArticleProjection::VT_HAS_SUMMARY, 22);
        assert_eq!(ArticleProjection::VT_SUMMARY, 24);
        assert_eq!(ArticleProjection::VT_HAS_HERO_IMAGE_URL, 26);
        assert_eq!(ArticleProjection::VT_HERO_IMAGE_URL, 28);
        assert_eq!(ArticleProjection::VT_D_TAG, 30);
        assert_eq!(ArticleProjection::VT_CONTENT_TREE, 32);
    }

    #[test]
    fn highlight_projection_offsets_are_stable() {
        assert_eq!(HighlightProjection::VT_ID, 4);
        assert_eq!(HighlightProjection::VT_AUTHOR_PUBKEY, 6);
        // Slots 8/10 are the deprecated author-display placeholder.
        assert_eq!(HighlightProjection::VT_CREATED_AT, 12);
        assert_eq!(HighlightProjection::VT_HIGHLIGHTED_TEXT, 14);
    }

    #[test]
    fn unknown_projection_offsets_are_stable() {
        assert_eq!(UnknownProjection::VT_KIND, 4);
        assert_eq!(UnknownProjection::VT_AUTHOR_PUBKEY, 6);
        // Slots 8/10/12/14 are the deprecated author-display placeholders.
        assert_eq!(UnknownProjection::VT_CREATED_AT, 16);
        assert_eq!(UnknownProjection::VT_CONTENT, 18);
        assert_eq!(UnknownProjection::VT_CONTENT_TREE, 20);
    }
}

// ── End-to-end article repro (#3016) ────────────────────────────────────────
//
// Reproduces the exact tag shape from the issue repro (kind:30023, title +
// summary + image + d tags) through the REAL resolver, proving `title` and
// `hero_image_url` decode as DISTINCT values (never swapped) after the wire
// round trip a typed-frame host performs.
#[test]
fn article_title_and_hero_image_never_swap_after_wire_round_trip() {
    let ev = kernel_event(
        "71e91e8cd84ec5503e15ed54812fc89feb2febf4e562cd2eecc84fdcf553cb1b",
        &"aa".repeat(32),
        30023,
        "body",
        vec![
            vec!["title".to_string(), "Chirp iOS Sweep Test Article".to_string()],
            vec![
                "summary".to_string(),
                "A short summary for the S26 article-embed render test.".to_string(),
            ],
            vec![
                "image".to_string(),
                "https://robohash.org/articletest.png?set=set4".to_string(),
            ],
            vec!["d".to_string(), "chirp-test-article-s26".to_string()],
        ],
    );
    // Register the real NIP-23 adapter exactly as the app does at startup —
    // no bespoke test-only Article construction, this exercises the whole
    // dispatch → tag-extraction → wire round trip a host actually sees.
    crate::register_article_projection_adapter(|event, content_tree| {
        Some(ArticleProjection {
            id: event.id.clone(),
            author_pubkey: event.author.clone(),
            created_at: event.created_at,
            title: event
                .tags
                .iter()
                .find(|t| t.first().map(String::as_str) == Some("title"))
                .and_then(|t| t.get(1).cloned()),
            summary: event
                .tags
                .iter()
                .find(|t| t.first().map(String::as_str) == Some("summary"))
                .and_then(|t| t.get(1).cloned()),
            hero_image_url: event
                .tags
                .iter()
                .find(|t| t.first().map(String::as_str) == Some("image"))
                .and_then(|t| t.get(1).cloned()),
            d_tag: event
                .tags
                .iter()
                .find(|t| t.first().map(String::as_str) == Some("d"))
                .and_then(|t| t.get(1).cloned())
                .unwrap_or_default(),
            content_tree,
        })
    });

    let mut entries = BTreeMap::new();
    entries.insert("art1".to_string(), envelope("art1", resolve(&ev)));
    let bytes = encode_ref_event_envelopes(&entries);
    let decoded = decode_ref_event_envelopes(&bytes).expect("decode");

    match &decoded.get("art1").unwrap().projection {
        EmbedKindProjection::Article(a) => {
            assert_eq!(
                a.title.as_deref(),
                Some("Chirp iOS Sweep Test Article"),
                "title must be the `title` tag, never the `image` tag"
            );
            assert_eq!(
                a.hero_image_url.as_deref(),
                Some("https://robohash.org/articletest.png?set=set4")
            );
            assert_ne!(a.title, a.hero_image_url, "title/hero must never collide");
            assert_eq!(a.d_tag, "chirp-test-article-s26");
        }
        other => panic!("expected Article, got {other:?}"),
    }
}
