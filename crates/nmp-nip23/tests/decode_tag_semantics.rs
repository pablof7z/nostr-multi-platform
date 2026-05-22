#![cfg(feature = "long-form")]
//! Coverage for NIP-23 tag semantics the existing suite leaves implicit:
//!
//! - **Multiple `t` hashtag tags** — NIP-12 allows an article to carry several
//!   `t` topic tags. The decoder preserves `tags` verbatim (no dedup, no
//!   collapse); `decode_full_article.rs` only proves *one* `t` survives. A
//!   regression that deduped or kept-first would silently drop topics.
//! - **Addressable coordinate (`a`-tag) reconstruction** — `nmp-nip23` exposes
//!   no `a`-tag builder; the NIP-01 coordinate `30023:<author>:<d_tag>` is
//!   instead reconstructable from `ArticleRecord` fields. These tests pin that
//!   the record carries exactly the three components an `a` reference needs.
//! - **First-`title` precedence at the public `try_from_event` boundary** — the
//!   `decode.rs` inline test asserts `first_tag_value` directly; this asserts
//!   the same first-wins rule through the decoder a consumer actually calls.

mod common;

use common::{article, stored};
use nmp_nip23::{try_from_event, ArticleRecord, KIND_LONG_FORM_ARTICLE};

const AUTHOR: &str = "feedface00000000000000000000000000000000000000000000000000000000";

/// Reconstruct the NIP-01 addressable-event coordinate (`a`-tag value) from a
/// decoded record: `<kind>:<author-pubkey>:<d-tag>`. This is the string a
/// *referencing* event would put in its `a` tag to point at this article.
fn addressable_coordinate(record: &ArticleRecord) -> String {
    format!("{KIND_LONG_FORM_ARTICLE}:{}:{}", record.author, record.d_tag)
}

#[test]
fn decoder_preserves_every_t_hashtag_tag_without_dedup() {
    // Three `t` tags, two of them identical, plus one distinct. A correct
    // decoder keeps the raw tag vector intact: no dedup, no collapse, order
    // preserved. NIP-23 articles routinely carry multiple topics this way.
    let event = stored(
        &"a".repeat(64),
        AUTHOR,
        30023,
        1_700_000_000,
        vec![
            vec!["d".into(), "multi-topic".into()],
            vec!["t".into(), "rust".into()],
            vec!["t".into(), "nostr".into()],
            vec!["t".into(), "rust".into()],
        ],
        "body",
    );

    let record = try_from_event(&event).expect("article with several t tags decodes");

    let topics: Vec<&str> = record
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("t"))
        .filter_map(|t| t.get(1))
        .map(String::as_str)
        .collect();

    // All three survive in original order — the duplicate `rust` is NOT deduped.
    assert_eq!(
        topics,
        vec!["rust", "nostr", "rust"],
        "every t tag must survive verbatim — no dedup, no reorder"
    );
}

#[test]
fn decoder_keeps_first_title_when_title_tag_duplicated() {
    // NIP-23 says nothing about multiple `title` tags; the decoder's documented
    // contract is "first tag value". Pin that through the public boundary so a
    // future "last wins" / "join" refactor is caught.
    let event = stored(
        &"a".repeat(64),
        AUTHOR,
        30023,
        0,
        vec![
            vec!["d".into(), "dup-title".into()],
            vec!["title".into(), "Canonical Title".into()],
            vec!["title".into(), "Shadow Title".into()],
        ],
        "",
    );

    let record = try_from_event(&event).unwrap();
    assert_eq!(
        record.title.as_deref(),
        Some("Canonical Title"),
        "first title tag wins; the later one is ignored"
    );
}

#[test]
fn record_reconstructs_the_nip01_addressable_coordinate() {
    // An article is a parameterized-replaceable event; other events point at it
    // with an `a` tag of the form `30023:<pubkey>:<d_tag>`. The decoder does
    // not emit that string, but `ArticleRecord` must carry every component so a
    // referencing-event builder can assemble it without a store round trip.
    let event = article(
        &"a".repeat(64),
        AUTHOR,
        1_700_000_000,
        "addressable-id",
        Some("Addressable"),
        Some(1_690_000_000),
        "body",
    );

    let record = try_from_event(&event).expect("article decodes");
    let coord = addressable_coordinate(&record);

    assert_eq!(coord, format!("30023:{AUTHOR}:addressable-id"));
    // The three colon-separated fields are exactly kind / author / d_tag.
    let parts: Vec<&str> = coord.splitn(3, ':').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "30023");
    assert_eq!(parts[1], record.author);
    assert_eq!(parts[2], record.d_tag);
}

#[test]
fn addressable_coordinate_is_stable_across_republish() {
    // NIP-33 replaceability: republishing the same `(author, d_tag)` produces a
    // new event id and `created_at` but the addressable coordinate is
    // identical — that stability is the whole point of the `d` tag. A consumer
    // that bookmarked the `a` coordinate must still resolve the article after a
    // republish.
    let first = article(&"a".repeat(64), AUTHOR, 100, "essay", None, None, "v1");
    let republished = article(&"b".repeat(64), AUTHOR, 200, "essay", None, None, "v2");

    let r1 = try_from_event(&first).unwrap();
    let r2 = try_from_event(&republished).unwrap();

    assert_ne!(r1.event_id, r2.event_id, "republish mints a new event id");
    assert_ne!(r1.created_at, r2.created_at, "republish bumps created_at");
    assert_eq!(
        addressable_coordinate(&r1),
        addressable_coordinate(&r2),
        "the a-tag coordinate is invariant under republish — that is the d tag's contract"
    );
}
