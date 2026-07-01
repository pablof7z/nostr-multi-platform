//! Proof tests for the NIP-23 long-form **typed** snapshot projection.
//!
//! The kernel fires `on_kernel_event` ONLY for store outcomes `Inserted |
//! Replaced` (supersession already resolved). These tests drive the observer
//! the same way the kernel's fan-out does, then **decode the typed
//! FlatBuffers sidecar payload back to its struct** (NOT a JSON map) and assert
//! the winning `ArticleProjection` is present, the superseded older event is
//! gone, fields are populated, and the projection is scoped (D5).

use super::*;
use crate::wire::longform_fb::{decode_longform_articles, LongformArticles};
use nmp_content::embed_projection::{resolve_embed_projection, EmbedKindProjection};
use nmp_content::{tokenize_with_kind, RenderContext, RenderMode};
use nmp_core::substrate::KernelEvent;

const AUTHOR_A: &str = "aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11a";
const AUTHOR_B: &str = "bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22b";

/// Build the address coordinate via the canonical identity primitive (not a
/// hand-rolled `format!`), so these tests cannot pass by mirroring a buggy wire
/// string — they assert against the same `AddressCoordinate` the projection key
/// is built from.
fn addr(author: &str, d_tag: &str) -> String {
    nmp_nip09::AddressCoordinate::new(KIND_LONG_FORM_ARTICLE, author, d_tag).to_wire()
}

fn article_event(
    id: &str,
    author: &str,
    d_tag: &str,
    created_at: u64,
    title: &str,
    summary: &str,
    image: &str,
    body: &str,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_LONG_FORM_ARTICLE,
        created_at,
        tags: vec![
            vec!["d".to_string(), d_tag.to_string()],
            vec!["title".to_string(), title.to_string()],
            vec!["summary".to_string(), summary.to_string()],
            vec!["image".to_string(), image.to_string()],
        ],
        content: body.to_string(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn article_embed_adapter_owns_nip23_tag_semantics() {
    let event = article_event(
        &"9".repeat(64),
        AUTHOR_A,
        "adapter-proof",
        1_234,
        "Adapter Title",
        "Adapter summary",
        "https://img.example/adapter.png",
        "body",
    );
    let content_tree =
        tokenize_with_kind(&event.content, &event.tags, RenderMode::Auto, event.kind).to_wire();
    let article =
        article_embed_projection_from_event(&event, content_tree).expect("article projection");

    assert_eq!(article.id, "9".repeat(64));
    assert_eq!(article.author_pubkey, AUTHOR_A);
    assert_eq!(article.title.as_deref(), Some("Adapter Title"));
    assert_eq!(article.summary.as_deref(), Some("Adapter summary"));
    assert_eq!(
        article.hero_image_url.as_deref(),
        Some("https://img.example/adapter.png")
    );
    assert_eq!(article.d_tag, "adapter-proof");
}

#[test]
fn registered_adapter_drives_nmp_content_article_embed_dispatch() {
    register_content_embed_projection_adapter();
    let event = article_event(
        &"8".repeat(64),
        AUTHOR_A,
        "registered",
        1_235,
        "Registered Title",
        "Registered summary",
        "https://img.example/registered.png",
        "body",
    );

    match resolve_embed_projection(&event, &RenderContext::new()) {
        EmbedKindProjection::Article(article) => {
            assert_eq!(article.title.as_deref(), Some("Registered Title"));
            assert_eq!(article.d_tag, "registered");
        }
        other => panic!("expected registered NIP-23 adapter to return Article, got {other:?}"),
    }
}

/// Decode the projection's typed sidecar payload back into the round-trip
/// struct — the load-bearing assertion that output is a TYPED FlatBuffer, not a
/// JSON `Value`.
fn decode_snapshot(p: &LongformProjection) -> LongformArticles {
    let entry = p.typed_projection();
    // Schema identity the host's NL23 decoder keys off.
    assert_eq!(entry.key, "nmp.nip23.articles");
    assert_eq!(entry.schema_id, "nmp.nip23.articles");
    assert_eq!(entry.file_identifier, "NL23");
    decode_longform_articles(&entry.payload).expect("typed sidecar payload must decode as NL23")
}

/// The headline proof: two kind:30023 for the SAME (author, d_tag) at different
/// `created_at` plus one unrelated article. The newest supersedes the older,
/// the typed `ArticleProjection` carries populated fields, the body round-trips
/// as a typed `ContentTreeWire`, and the snapshot is scoped to the observed set.
#[test]
fn supersession_keeps_newest_typed_article_and_is_scoped() {
    let projection = LongformProjection::new();

    // NEWER event for (AUTHOR_A, "rust-guide") arrives FIRST, then the older
    // event arrives LATE. This is the falsifiable case: a plain last-write-wins
    // map would clobber the newer with the older. The created_at-guarded collapse
    // must keep the newer winner regardless of arrival order.
    projection.on_kernel_event(&article_event(
        &"2".repeat(64),
        AUTHOR_A,
        "rust-guide",
        2_000,
        "New Title",
        "New summary",
        "https://img.example/new.png",
        "new body",
    ));
    projection.on_kernel_event(&article_event(
        &"1".repeat(64),
        AUTHOR_A,
        "rust-guide",
        1_000,
        "Old Title",
        "Old summary",
        "https://img.example/old.png",
        "old body",
    ));
    // Unrelated article (different author + d_tag) — must coexist, proving the
    // map is keyed by the addressable coordinate, not flattened.
    projection.on_kernel_event(&article_event(
        &"3".repeat(64),
        AUTHOR_B,
        "nostr-intro",
        1_500,
        "Nostr Intro",
        "An intro",
        "https://img.example/nostr.png",
        "intro body",
    ));

    let snap = decode_snapshot(&projection);

    // Scope: exactly two coordinates survive — the superseded older one is gone.
    assert_eq!(
        snap.articles.len(),
        2,
        "feed holds one row per surviving coordinate"
    );
    assert_eq!(
        snap.documents.len(),
        2,
        "documents hold one entry per surviving coordinate"
    );

    let addr_a = addr(AUTHOR_A, "rust-guide");
    let addr_b = addr(AUTHOR_B, "nostr-intro");

    // The winning (newest) article replaced the older one under the same key.
    let doc_a = snap.documents.get(&addr_a).expect("AUTHOR_A document");
    assert_eq!(doc_a.id, "2".repeat(64));
    assert_eq!(doc_a.title.as_deref(), Some("New Title"));
    assert_eq!(doc_a.created_at, 2_000);

    // The older event's id must NOT appear anywhere.
    let old_id = "1".repeat(64);
    assert!(
        !snap.documents.values().any(|d| d.id == old_id),
        "superseded older event must be gone"
    );

    // Newest-first ordering in the feed list.
    assert_eq!(snap.articles[0].address, addr_a);
    assert_eq!(snap.articles[1].address, addr_b);

    // Typed feed-summary fields are populated (title/summary/image/author).
    let row_a = &snap.articles[0];
    assert_eq!(row_a.title, "New Title");
    assert_eq!(row_a.summary, "New summary");
    assert_eq!(row_a.hero_image_url, "https://img.example/new.png");
    assert_eq!(row_a.author_pubkey, AUTHOR_A);
    assert_eq!(row_a.d_tag, "rust-guide");

    // The full document carries the typed content tree body (round-tripped via
    // the existing NFCT codec, not re-parsed) — the body is non-empty.
    assert!(
        !doc_a.content_tree.nodes.is_empty() || !doc_a.content_tree.roots.is_empty(),
        "open document carries the rendered article body"
    );
}

/// Non-30023 events are ignored — the projection only ever holds articles (D5
/// scope discipline; an unrelated kind on the same observer stream is a no-op).
#[test]
fn ignores_non_article_kinds() {
    let projection = LongformProjection::new();
    let note = KernelEvent {
        id: "f".repeat(64),
        author: AUTHOR_A.to_string(),
        kind: 1,
        created_at: 1_000,
        tags: vec![],
        content: "just a note".to_string(),
        relay_provenance: Vec::new(),
    };
    projection.on_kernel_event(&note);

    let snap = decode_snapshot(&projection);
    assert!(snap.articles.is_empty());
    assert!(snap.documents.is_empty());
}

/// An empty projection is a well-formed, present typed buffer (D1: never absent
/// at the boundary — even with nothing to show, the payload decodes cleanly).
#[test]
fn empty_projection_is_well_formed() {
    let projection = LongformProjection::new();
    let snap = decode_snapshot(&projection);
    assert!(snap.articles.is_empty());
    assert!(snap.documents.is_empty());
}

/// A missing `title`/`summary`/`image` tag yields empty-string placeholders in
/// the feed summary (D1), not a hidden row — and `None` (absent), not a present
/// empty string, in the full document (raw protocol data preserved).
#[test]
fn missing_tags_become_placeholders_in_feed_and_none_in_document() {
    let projection = LongformProjection::new();
    let bare = KernelEvent {
        id: "e".repeat(64),
        author: AUTHOR_A.to_string(),
        kind: KIND_LONG_FORM_ARTICLE,
        created_at: 1_000,
        tags: vec![vec!["d".to_string(), "bare".to_string()]],
        content: "body only".to_string(),
        relay_provenance: Vec::new(),
    };
    projection.on_kernel_event(&bare);

    let snap = decode_snapshot(&projection);
    assert_eq!(snap.articles.len(), 1);
    let row = &snap.articles[0];
    // Feed summary: empty-string placeholders (D1 — never hide the row).
    assert_eq!(row.title, "");
    assert_eq!(row.summary, "");
    assert_eq!(row.hero_image_url, "");

    // Full document: raw `Option` preserved as `None` (the `has_*` presence flag
    // round-trips absent distinctly from a present empty string).
    let addr = addr(AUTHOR_A, "bare");
    let doc = snap.documents.get(&addr).expect("document present");
    assert_eq!(doc.title, None);
    assert_eq!(doc.summary, None);
    assert_eq!(doc.hero_image_url, None);
}

fn delete_event(author: &str, created_at: u64, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: "d".repeat(64),
        author: author.to_string(),
        kind: nmp_nip18::KIND_DELETE,
        created_at,
        tags: tags
            .into_iter()
            .map(|t| t.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn kind5_coordinate_delete_by_owner_removes_stored_article() {
    // The typed projection must not keep serving a coordinate the author
    // retracted (issue #1740 step 5) — otherwise the sidecar contradicts the
    // feed, which already dropped the row.
    let projection = LongformProjection::new();
    projection.on_kernel_event(&article_event(
        &"1".repeat(64),
        AUTHOR_A,
        "rust-guide",
        1_000,
        "Title",
        "Summary",
        "https://img.example/x.png",
        "body",
    ));
    assert_eq!(decode_snapshot(&projection).articles.len(), 1);

    let addr = addr(AUTHOR_A, "rust-guide");
    projection.on_kernel_event(&delete_event(AUTHOR_A, 2_000, vec![vec!["a", &addr]]));

    assert!(
        decode_snapshot(&projection).articles.is_empty(),
        "owner a-tag delete retracts the stored coordinate"
    );
}

#[test]
fn kind5_coordinate_delete_by_foreign_author_is_noop() {
    let projection = LongformProjection::new();
    projection.on_kernel_event(&article_event(
        &"1".repeat(64),
        AUTHOR_A,
        "rust-guide",
        1_000,
        "Title",
        "Summary",
        "https://img.example/x.png",
        "body",
    ));

    let addr = addr(AUTHOR_A, "rust-guide");
    // AUTHOR_B does not own AUTHOR_A's coordinate.
    projection.on_kernel_event(&delete_event(AUTHOR_B, 2_000, vec![vec!["a", &addr]]));

    assert_eq!(
        decode_snapshot(&projection).articles.len(),
        1,
        "a foreign delete must not retract someone else's coordinate"
    );
}
