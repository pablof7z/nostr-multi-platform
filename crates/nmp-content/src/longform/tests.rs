//! Proof tests for the NIP-23 long-form typed projection.
//!
//! The kernel fires `on_kernel_event` ONLY for store outcomes `Inserted |
//! Replaced` (supersession already resolved). These tests drive the observer
//! the same way the kernel's fan-out does and assert the typed
//! `ArticleProjection` lands in the snapshot, the superseded older event is
//! gone, the newest wins, fields are populated, and the projection is scoped to
//! the events it was actually shown (D5).

use super::*;
use serde_json::Value;

const AUTHOR_A: &str = "aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11a";
const AUTHOR_B: &str = "bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22b";

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
    }
}

/// Decode the projection snapshot into its two sub-objects.
fn snapshot_parts(p: &LongformProjection) -> (Vec<Value>, serde_json::Map<String, Value>) {
    let snap = p.snapshot_json();
    let articles = snap
        .get("articles")
        .and_then(Value::as_array)
        .cloned()
        .expect("articles array present");
    let documents = snap
        .get("documents")
        .and_then(Value::as_object)
        .cloned()
        .expect("documents object present");
    (articles, documents)
}

/// The headline proof: two kind:30023 for the SAME (author, d_tag) at different
/// `created_at` plus one unrelated article. The newest supersedes the older,
/// the typed `ArticleProjection` carries populated fields, and the snapshot is
/// scoped to exactly the observed set.
#[test]
fn supersession_keeps_newest_typed_article_and_is_scoped() {
    let projection = LongformProjection::new();

    // Older event for (AUTHOR_A, "rust-guide").
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
    // Newer event for the SAME coordinate — supersedes the older one. (The
    // kernel only fires us with the winner; firing both here proves
    // last-write-wins converges to the newest regardless of arrival order.)
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

    let (articles, documents) = snapshot_parts(&projection);

    // Scope: exactly two coordinates survive — the superseded older one is gone.
    assert_eq!(articles.len(), 2, "feed holds one row per surviving coordinate");
    assert_eq!(documents.len(), 2, "documents hold one entry per surviving coordinate");

    let addr_a = format!("{KIND_LONG_FORM_ARTICLE}:{AUTHOR_A}:rust-guide");
    let addr_b = format!("{KIND_LONG_FORM_ARTICLE}:{AUTHOR_B}:nostr-intro");

    // The winning (newest) article replaced the older one under the same key.
    let doc_a = documents.get(&addr_a).expect("AUTHOR_A document present");
    assert_eq!(doc_a.get("id").and_then(Value::as_str), Some("2".repeat(64).as_str()));
    assert_eq!(doc_a.get("title").and_then(Value::as_str), Some("New Title"));
    assert_eq!(doc_a.get("createdAt").and_then(Value::as_u64), Some(2_000));
    // The older event's id must NOT appear anywhere.
    let old_id = "1".repeat(64);
    assert!(
        !documents.values().any(|d| d.get("id").and_then(Value::as_str) == Some(old_id.as_str())),
        "superseded older event must be gone"
    );

    // Newest-first ordering in the feed list.
    assert_eq!(articles[0].get("address").and_then(Value::as_str), Some(addr_a.as_str()));
    assert_eq!(articles[1].get("address").and_then(Value::as_str), Some(addr_b.as_str()));

    // Typed feed-summary fields are populated (title/summary/image/author).
    let row_a = &articles[0];
    assert_eq!(row_a.get("title").and_then(Value::as_str), Some("New Title"));
    assert_eq!(row_a.get("summary").and_then(Value::as_str), Some("New summary"));
    assert_eq!(
        row_a.get("heroImageUrl").and_then(Value::as_str),
        Some("https://img.example/new.png")
    );
    assert_eq!(row_a.get("authorPubkey").and_then(Value::as_str), Some(AUTHOR_A));
    assert_eq!(row_a.get("dTag").and_then(Value::as_str), Some("rust-guide"));

    // D5: the feed-summary row carries NO content_tree (only the document does).
    assert!(row_a.get("contentTree").is_none(), "feed summary must omit the article body");
    // The full document DOES carry the typed content tree.
    assert!(doc_a.get("contentTree").is_some(), "open document carries the article body");
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
    };
    projection.on_kernel_event(&note);

    let (articles, documents) = snapshot_parts(&projection);
    assert!(articles.is_empty());
    assert!(documents.is_empty());
}

/// An empty projection is a well-formed, present `{articles:[],documents:{}}`
/// (D1: never absent / null at the boundary).
#[test]
fn empty_projection_is_well_formed() {
    let projection = LongformProjection::new();
    let snap = projection.snapshot_json();
    assert_eq!(snap.get("articles").and_then(Value::as_array).map(Vec::len), Some(0));
    assert_eq!(snap.get("documents").and_then(Value::as_object).map(|m| m.len()), Some(0));
}

/// A missing `title`/`summary`/`image` tag yields empty-string placeholders in
/// the feed summary (D1), not a hidden row or a `null`.
#[test]
fn missing_tags_become_placeholders_not_optionals() {
    let projection = LongformProjection::new();
    let bare = KernelEvent {
        id: "e".repeat(64),
        author: AUTHOR_A.to_string(),
        kind: KIND_LONG_FORM_ARTICLE,
        created_at: 1_000,
        tags: vec![vec!["d".to_string(), "bare".to_string()]],
        content: "body only".to_string(),
    };
    projection.on_kernel_event(&bare);

    let (articles, _) = snapshot_parts(&projection);
    assert_eq!(articles.len(), 1);
    let row = &articles[0];
    assert_eq!(row.get("title").and_then(Value::as_str), Some(""));
    assert_eq!(row.get("summary").and_then(Value::as_str), Some(""));
    assert_eq!(row.get("heroImageUrl").and_then(Value::as_str), Some(""));
}
