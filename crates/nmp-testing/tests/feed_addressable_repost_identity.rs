//! Catalog slots E02/E03/H06 — replaceable/addressable repost identity
//! (issue #1740 step 5).
//!
//! These are the cross-crate acceptance gates for the centralized
//! address-coordinate target identity. The unit tests in `nmp-nip18`,
//! `nmp-content`, and `nmp-nip68` cover the decode/feed seams; this file binds
//! the contract end-to-end through the app-neutral long-form feed adapter and
//! the canonical `nmp_nip18` identity primitives:
//!
//! * **E02** — versions at one coordinate collapse to a single row; a newer
//!   event at `(pubkey, kind, d)` supersedes the older. Reposts and the direct
//!   article at the same coordinate are one row, not several.
//! * **E03** — an event-id-only wrapper that cannot resolve to a coordinate
//!   stays UNRESOLVED / fail-closed; the feed never fabricates a coordinate
//!   from an event id, so no row appears.
//! * **H06** — a kind:5 deletion removes a row only when it can prove the
//!   target: an `a`-tag coordinate owned by the delete author, or an `e`-tag
//!   event id owned by the delete author. A foreign delete or an unresolvable
//!   target is a no-op.

use std::sync::Arc;

use nmp_content::{longform_feed_predicate, LongformFeed, KIND_LONG_FORM_ARTICLE};
use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_feed::FeedRequest;
use nmp_nip18::{AddressCoordinate, KIND_GENERIC_REPOST, KIND_DELETE};

const AUTHOR_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REPOSTER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const FOREIGN: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn coordinate(author: &str, d_tag: &str) -> String {
    AddressCoordinate::new(KIND_LONG_FORM_ARTICLE, author, d_tag).to_wire()
}

fn article(id: &str, author: &str, d_tag: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_LONG_FORM_ARTICLE,
        created_at,
        tags: vec![
            vec!["d".to_string(), d_tag.to_string()],
            vec!["title".to_string(), format!("title {d_tag}")],
            vec!["t".to_string(), "nostr".to_string()],
        ],
        content: format!("body {d_tag}"),
        relay_provenance: vec!["wss://relay.example".to_string()],
    }
}

fn address_repost(id: &str, author: &str, coord: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_GENERIC_REPOST,
        created_at,
        tags: vec![
            vec!["a".to_string(), coord.to_string()],
            vec!["k".to_string(), KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn event_id_only_repost(id: &str, author: &str, target_id: &str, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_GENERIC_REPOST,
        created_at,
        tags: vec![
            vec!["e".to_string(), target_id.to_string()],
            vec!["k".to_string(), KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

/// A generic repost that embeds the full target article in `content`, so the
/// row hydrates the article body without a local lookup.
fn embedded_repost(id: &str, author: &str, target: &KernelEvent, created_at: u64) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_GENERIC_REPOST,
        created_at,
        tags: vec![
            vec!["e".to_string(), target.id.clone()],
            vec!["k".to_string(), KIND_LONG_FORM_ARTICLE.to_string()],
        ],
        content: serde_json::json!({
            "id": target.id,
            "pubkey": target.author,
            "kind": target.kind,
            "created_at": target.created_at,
            "tags": target.tags,
            "content": target.content,
        })
        .to_string(),
        relay_provenance: Vec::new(),
    }
}

fn delete(id: &str, author: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: KIND_DELETE,
        created_at: 1_000,
        tags: tags
            .into_iter()
            .map(|t| t.into_iter().map(str::to_string).collect())
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn open_feed() -> Arc<LongformFeed> {
    LongformFeed::new(longform_feed_predicate(Arc::new(|_| true)))
}

/// H06 prerequisite — an addressable feed actually subscribes to kind:5.
///
/// The behavioral delete tests below replay kind:5 directly into the observer;
/// this guards the other half: the compiler-derived acquisition set for a
/// `[30023]` feed includes kind:5, so live subscriptions receive the deletes the
/// observer acts on. Without it the delete handling would be dead in production.
#[test]
fn h06_addressable_feed_acquires_kind5_deletes() {
    let kinds = nmp_content::longform_acquisition_kinds();
    assert!(
        kinds.contains(&KIND_DELETE),
        "a [30023] feed must acquire kind:5 so deletes reach the observer; got {kinds:?}"
    );
    assert!(kinds.contains(&KIND_GENERIC_REPOST));
    assert!(kinds.contains(&KIND_LONG_FORM_ARTICLE));

    // kind:5 is derived acquisition, never a primary app input.
    assert!(
        nmp_nip18::try_acquisition_kinds_for_primary([KIND_DELETE]).is_err(),
        "kind:5 must fail closed as a primary feed kind"
    );
}

/// E02 — a direct article and an `a`-tag repost at the same coordinate collapse
/// to one row, and a newer version supersedes the older. Versions do not stack.
#[test]
fn e02_versions_at_one_coordinate_collapse_to_a_single_row() {
    let feed = open_feed();
    let coord = coordinate(AUTHOR_A, "article");

    let v1 = article("v1", AUTHOR_A, "article", 10);
    feed.on_kernel_event(&v1);
    feed.on_kernel_event(&address_repost("repost", REPOSTER, &coord, 40));

    // One row even though two distinct events (article + repost) name it.
    assert_eq!(feed.len(), 1, "repost and article collapse to one coordinate row");
    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards[0].card.id, coord);
    assert_eq!(snapshot.cards[0].card.article.as_ref().unwrap().id, "v1");

    // A newer version at the same coordinate supersedes the older body.
    let v2 = article("v2", AUTHOR_A, "article", 90);
    feed.on_kernel_event(&v2);
    assert_eq!(feed.len(), 1, "newer version does not add a second row");
    let snapshot = feed.snapshot(&FeedRequest::default());
    assert_eq!(
        snapshot.cards[0].card.article.as_ref().unwrap().id,
        "v2",
        "latest-at-coordinate wins"
    );
}

/// E03 — an event-id-only wrapper cannot resolve a coordinate and stays
/// UNRESOLVED. The feed renders no row and never guesses a coordinate.
#[test]
fn e03_event_id_only_wrapper_stays_unresolved_fail_closed() {
    let feed = open_feed();

    // No embedded body, no local lookup, only an `e` tag → no proven coordinate.
    feed.on_kernel_event(&event_id_only_repost("repost", REPOSTER, "unknown-target", 40));

    assert!(
        feed.is_empty(),
        "an event id must never be fabricated into a coordinate row"
    );

    // The decoder also refuses to invent a coordinate from the event id.
    let record = nmp_nip18::try_from_kernel_event(&event_id_only_repost(
        "repost",
        REPOSTER,
        "unknown-target",
        40,
    ))
    .unwrap();
    assert_eq!(record.target_event_id.as_deref(), Some("unknown-target"));
    assert_eq!(
        record.target_address, None,
        "no coordinate is guessed from an event id"
    );
}

/// H06 — a kind:5 `a`-tag delete owned by the coordinate author removes the row.
#[test]
fn h06_kind5_coordinate_delete_by_owner_removes_row() {
    let feed = open_feed();
    feed.on_kernel_event(&article("v1", AUTHOR_A, "article", 10));
    assert_eq!(feed.len(), 1);

    let coord = coordinate(AUTHOR_A, "article");
    feed.on_kernel_event(&delete("del", AUTHOR_A, vec![vec!["a", &coord]]));

    assert!(feed.is_empty(), "owner a-tag delete removes the coordinate row");
}

/// H06 negative — a foreign kind:5 `a`-tag delete is a no-op.
#[test]
fn h06_kind5_coordinate_delete_by_foreign_author_is_noop() {
    let feed = open_feed();
    feed.on_kernel_event(&article("v1", AUTHOR_A, "article", 10));

    let coord = coordinate(AUTHOR_A, "article");
    feed.on_kernel_event(&delete("del", FOREIGN, vec![vec!["a", &coord]]));

    assert_eq!(feed.len(), 1, "a foreign delete cannot remove the coordinate row");
}

/// H06 — a kind:5 `e`-tag delete owned by the source author removes that row.
#[test]
fn h06_kind5_event_id_delete_removes_owned_source_row() {
    let feed = open_feed();
    feed.on_kernel_event(&article("v1", AUTHOR_A, "article", 10));
    assert_eq!(feed.len(), 1);

    feed.on_kernel_event(&delete("del", AUTHOR_A, vec![vec!["e", "v1"]]));

    assert!(feed.is_empty(), "owner e-tag delete removes the source row");
}

/// H06 negative — a kind:5 whose only target is unresolvable changes nothing.
#[test]
fn h06_kind5_unresolvable_target_is_noop() {
    let feed = open_feed();
    feed.on_kernel_event(&article("v1", AUTHOR_A, "article", 10));

    // `a` for a non-addressable kind (no coordinate), `e` for an absent event.
    feed.on_kernel_event(&delete(
        "del",
        AUTHOR_A,
        vec![vec!["a", "1:aaaa:x"], vec!["e", "no-such-event"]],
    ));

    assert_eq!(feed.len(), 1, "an unresolvable delete is a no-op");
}

/// H06 — author A's `e`-tag delete of A's article event clears the article even
/// when it is surfaced only through reposter C's wrapper (whose event id
/// differs). A retracted article must not live on through a repost.
#[test]
fn h06_kind5_event_id_delete_clears_deleted_article_surfaced_via_repost() {
    let feed = open_feed();
    let target = article("art", AUTHOR_A, "article", 10);
    // Only C's repost positions the row; A's article event itself is not in the
    // feed directly (the row's source is the wrapper "wrap").
    feed.on_kernel_event(&embedded_repost("wrap", REPOSTER, &target, 40));
    assert_eq!(feed.len(), 1);
    assert_eq!(
        feed.snapshot(&FeedRequest::default()).cards[0]
            .card
            .article
            .as_ref()
            .unwrap()
            .id,
        "art"
    );

    // A deletes its own article event id. The wrapper id ("wrap") differs, but
    // the deleted article body (id "art", author A) must be removed.
    feed.on_kernel_event(&delete("del", AUTHOR_A, vec![vec!["e", "art"]]));

    assert!(
        feed.is_empty(),
        "A's deleted article must not survive through C's repost"
    );
}

/// H06 negative — a FOREIGN account cannot use an `e`-tag delete naming A's
/// article id to strip A's article from a repost (NIP-09 ownership).
#[test]
fn h06_kind5_event_id_delete_through_repost_is_noop_for_foreign_author() {
    let feed = open_feed();
    let target = article("art", AUTHOR_A, "article", 10);
    feed.on_kernel_event(&embedded_repost("wrap", REPOSTER, &target, 40));
    assert_eq!(feed.len(), 1);

    // FOREIGN does not own article "art"; the delete is a no-op.
    feed.on_kernel_event(&delete("del", FOREIGN, vec![vec!["e", "art"]]));

    assert_eq!(
        feed.len(),
        1,
        "only the article's own author may retract it through a repost"
    );
}
