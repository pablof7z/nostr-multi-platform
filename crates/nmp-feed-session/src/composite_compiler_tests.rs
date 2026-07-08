//! Driving-example proof (#3082 settled design): a composite feed of kind:30023
//! articles surfaced via THREE lanes — authored (Direct), commented
//! (`nip22.root`), reposted (`nip18.target`) — all by a follows author set,
//! deduped to ONE row per article, provenance accumulating
//! `{Authored, CommentedBy, RepostedBy}`, and the article's real `created_at`
//! available once delivered.
//!
//! This test exercises the REAL mechanism end to end at the layer where it
//! actually lives: the REAL `nmp_feed::FlatFeed` engine (Change 1 — arity-`Vec`
//! item builder), the REAL `nmp-nip18`/`nmp-nip22` registered lane mappings,
//! and the REAL [`DeliveredRefDemand`] admission/shape primitive (Change 2 —
//! the delivery-tagged `TypedRef` vector). It deliberately bypasses
//! `FeedSessionHost`/acquisition-resolution — that plumbing is scope-agnostic
//! and already covered by this crate's `resolve_tests`/`trellis_adapter_tests`;
//! this test's job is to prove the NEW composite row-building + merge + demand
//! logic, not re-litigate the acquisition layer.
//!
//! ## Why this fails without the changes
//!
//! Under the demolished single-arity engine
//! (`FlatFeedItemBuilder: Fn(&KernelEvent) -> Option<FlatFeedItem<C>>`), one
//! event could produce AT MOST one row — so an event admitted by multiple
//! lanes (impossible to even express) could never fan out. And with no
//! `Delivered`-ref demand primitive, a comment/repost row had no path to the
//! real article's `created_at` other than a synchronous by-id store peek (the
//! #3083 cache-luck bug this design forecloses) or the app never learning it
//! at all. Both properties are asserted below.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::ObservedProjectionSink;
use nmp_feed::{FeedRequest, FeedRow, FeedRowContext, FlatFeed, SortPolicy};

use super::{build_composite_rows, composite_merge, lane_claims, tags_match, CompiledLane};
use crate::delivered_ref::{demand_admission, demand_live_shape, DeliveredRefDemand};

const KIND_ARTICLE: u32 = 30_023;
const KIND_COMMENT: u32 = 1111;
const KIND_REPOST: u32 = 16;

fn article(author: &str, d: &str, created_at: u64, id: &str, content: &str) -> KernelEvent {
    KernelEvent {
        id: EventId::from(id),
        author: author.to_string(),
        kind: KIND_ARTICLE,
        created_at,
        tags: vec![vec!["d".to_string(), d.to_string()]],
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

fn comment(author: &str, article_author: &str, d: &str, created_at: u64, id: &str) -> KernelEvent {
    KernelEvent {
        id: EventId::from(id),
        author: author.to_string(),
        kind: KIND_COMMENT,
        created_at,
        tags: vec![
            vec![
                "A".to_string(),
                format!("{KIND_ARTICLE}:{article_author}:{d}"),
            ],
            vec!["K".to_string(), KIND_ARTICLE.to_string()],
            vec![
                "a".to_string(),
                format!("{KIND_ARTICLE}:{article_author}:{d}"),
            ],
            vec!["k".to_string(), KIND_ARTICLE.to_string()],
        ],
        content: "great read".to_string(),
        relay_provenance: Vec::new(),
    }
}

fn repost(author: &str, article_author: &str, d: &str, created_at: u64, id: &str) -> KernelEvent {
    KernelEvent {
        id: EventId::from(id),
        author: author.to_string(),
        kind: KIND_REPOST,
        created_at,
        tags: vec![
            vec![
                "a".to_string(),
                format!("{KIND_ARTICLE}:{article_author}:{d}"),
            ],
            vec!["k".to_string(), KIND_ARTICLE.to_string()],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

/// The "authored" lane's mapping: what a real app/nip23 composition root would
/// register for an address-replaceable content kind — canonical id = the
/// event's own NIP-01 address coordinate (kind:pubkey:d), NOT `event.id` (the
/// generic `feed.authored` identity mapping is event-id-keyed, appropriate for
/// non-replaceable kinds; a replaceable kind's canonical identity is its own
/// coordinate so every revision — and every discovery lane pointing at it —
/// collapses onto the SAME row).
fn direct_article_mapping() -> nmp_feed::LaneMapping {
    Arc::new(|event: &KernelEvent| {
        let Some(d) = event
            .tags
            .iter()
            .find(|tag| tag.first().map(String::as_str) == Some("d"))
            .and_then(|tag| tag.get(1))
        else {
            return Vec::new();
        };
        vec![nmp_feed::MappedRow {
            canonical_row_id: format!("{}:{}:{}", event.kind, event.author, d),
            payload: nmp_feed::MappedPayload::FromEvent,
            context: vec![FeedRowContext::Authored],
            refs: Vec::new(),
        }]
    })
}

/// Build the driving example's 3-lane composite, all scoped to `follows`
/// (a static author set — the acquisition/`FeedScope::Authors` layer is
/// already proven elsewhere; here the lane admission is expressed directly as
/// the author-membership predicate that scope compiles to).
struct DrivingExample {
    feed: Arc<FlatFeed<FeedRow>>,
    demand: Arc<DeliveredRefDemand>,
}

fn driving_example(follows: BTreeSet<String>) -> DrivingExample {
    let lanes = vec![
        CompiledLane {
            admission: author_admission(follows.clone()),
            match_kinds: BTreeSet::from([KIND_ARTICLE]),
            match_tags: Default::default(),
            mapping: direct_article_mapping(),
        },
        CompiledLane {
            admission: author_admission(follows.clone()),
            match_kinds: BTreeSet::from([KIND_COMMENT]),
            match_tags: [("K".to_string(), BTreeSet::from([KIND_ARTICLE.to_string()]))]
                .into_iter()
                .collect(),
            mapping: nmp_nip22::nip22_root_mapping(),
        },
        CompiledLane {
            admission: author_admission(follows),
            match_kinds: BTreeSet::from([KIND_REPOST]),
            match_tags: [("k".to_string(), BTreeSet::from([KIND_ARTICLE.to_string()]))]
                .into_iter()
                .collect(),
            mapping: nmp_nip18::nip18_target_mapping(),
        },
    ];
    let lanes = Arc::new(lanes);
    let demand = DeliveredRefDemand::new();
    let render_target_kinds = vec![KIND_ARTICLE];

    let admission = {
        let lanes = Arc::clone(&lanes);
        let demand_admits = demand_admission(&demand, render_target_kinds.clone());
        Arc::new(move |event: &KernelEvent| {
            lanes.iter().any(|lane| lane_claims(lane, event)) || demand_admits(event)
        })
    };
    let item_builder = {
        let lanes = Arc::clone(&lanes);
        let demand = Arc::clone(&demand);
        let render_target_kinds = render_target_kinds.clone();
        Arc::new(move |event: &KernelEvent| {
            build_composite_rows(&lanes, &demand, &render_target_kinds, event)
        })
    };
    let merge = composite_merge(SortPolicy::ByTargetCreatedAt);

    let feed = FlatFeed::with_merge(admission, item_builder, None, merge);
    DrivingExample { feed, demand }
}

fn author_admission(authors: BTreeSet<String>) -> nmp_feed::RootAdmission {
    Arc::new(move |event: &KernelEvent| authors.contains(&event.author))
}

#[test]
fn composite_feed_dedupes_three_lanes_to_one_row_with_accumulated_provenance() {
    let author = "article-author".to_string();
    let commenter = "commenter".to_string();
    let reposter = "reposter".to_string();
    let follows: BTreeSet<String> = [author.clone(), commenter.clone(), reposter.clone()]
        .into_iter()
        .collect();

    let example = driving_example(follows);
    let d_tag = "my-article";
    let coordinate = format!("{KIND_ARTICLE}:{author}:{d_tag}");

    // Comment and repost arrive FIRST — before the article itself is
    // delivered. Change 2: each lane's `Delivered` ref registers demand with
    // the SAME `DeliveredRefDemand`, so the composite's OWN admission will
    // accept the article once it arrives (never `resolve_ref`, never a store
    // peek).
    example
        .feed
        .on_kernel_event(&comment(&commenter, &author, d_tag, 200, "comment-1"));
    example
        .feed
        .on_kernel_event(&repost(&reposter, &author, d_tag, 210, "repost-1"));

    // Change 1 in action: TWO different lanes (commented, reposted) each
    // contributed a row for the SAME canonical id from TWO different source
    // events — they deduped into one row. Provenance already accumulated from
    // both, and the demand model already knows to admit the real article.
    let snap = example.feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(snap.cards.len(), 1, "comment + repost dedupe to ONE row");
    let row = &snap.cards[0].card;
    assert_eq!(row.canonical_row_id, coordinate);
    assert!(
        row.is_placeholder(),
        "no lane has delivered the real article yet"
    );
    let contexts: BTreeSet<_> = row.context.iter().cloned().collect();
    assert_eq!(
        contexts,
        BTreeSet::from([
            FeedRowContext::CommentedBy {
                author_pubkey: commenter.clone(),
                comment_event_id: "comment-1".to_string(),
                comment_created_at: 200,
            },
            FeedRowContext::RepostedBy {
                author_pubkey: reposter.clone(),
                note_created_at: 210,
            },
        ]),
        "provenance SET accumulated across both provenance-only lanes"
    );
    assert!(
        example
            .demand
            .is_demanded(&nmp_feed::TypedRefTarget::Address {
                kind: KIND_ARTICLE,
                pubkey: author.clone(),
                d: d_tag.to_string(),
            }),
        "the article's own delivery is now demanded by this session"
    );

    // The article itself arrives — published LONG before either the comment
    // or the repost (a realistic "old article resurfaced" timeline). This is
    // authored by `author`, who is ALSO admitted directly by the `Direct`
    // lane — proving BOTH the delivered-ref admission AND the direct lane
    // converge on the SAME canonical row.
    let article_created_at = 50;
    example.feed.on_kernel_event(&article(
        &author,
        d_tag,
        article_created_at,
        "article-1",
        "the actual article body",
    ));

    let snap = example.feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(
        snap.cards.len(),
        1,
        "still ONE row — the article merged into the SAME canonical row"
    );
    let row = &snap.cards[0].card;
    assert_eq!(row.canonical_row_id, coordinate);
    assert!(!row.is_placeholder(), "the row is now hydrated");
    assert_eq!(row.content, "the actual article body");
    assert_eq!(row.author_pubkey, author);
    assert_eq!(
        row.created_at, article_created_at,
        "SortPolicy::ByTargetCreatedAt: the row's sort/created_at reflects the \
         article's OWN real created_at, not the comment/repost interaction time \
         — the whole point of a `Delivered` ref over a bare `RenderOnly` one"
    );
    let contexts: BTreeSet<_> = row.context.iter().cloned().collect();
    assert_eq!(
        contexts,
        BTreeSet::from([
            FeedRowContext::Authored,
            FeedRowContext::CommentedBy {
                author_pubkey: commenter,
                comment_event_id: "comment-1".to_string(),
                comment_created_at: 200,
            },
            FeedRowContext::RepostedBy {
                author_pubkey: reposter,
                note_created_at: 210,
            },
        ]),
        "provenance SET now accumulates all three: Authored + CommentedBy + RepostedBy"
    );
}

#[test]
fn delivered_ref_admits_the_article_even_when_the_direct_lane_would_not() {
    // The article's author is NOT in the direct lane's follows set — only the
    // commenter is. This isolates the claim that matters: the `Delivered` ref
    // demand mechanism ALONE (not a coincidental direct-lane admission) is
    // what pulls the real article in.
    let author = "outside-author".to_string();
    let commenter = "commenter".to_string();
    let follows: BTreeSet<String> = [commenter.clone()].into_iter().collect();

    let example = driving_example(follows);
    let d_tag = "solo-article";
    let coordinate = format!("{KIND_ARTICLE}:{author}:{d_tag}");

    example
        .feed
        .on_kernel_event(&comment(&commenter, &author, d_tag, 500, "comment-only"));
    assert_eq!(
        example.feed.snapshot(&FeedRequest::newest(10)).cards.len(),
        1
    );

    // The article's own event: its author is admitted ONLY via the demand
    // model (the direct lane's `author_admission` would reject it outright).
    example.feed.on_kernel_event(&article(
        &author,
        d_tag,
        42,
        "solo-article-event",
        "hydrated via delivered-ref demand alone",
    ));

    let snap = example.feed.snapshot(&FeedRequest::newest(10));
    assert_eq!(snap.cards.len(), 1);
    let row = &snap.cards[0].card;
    assert_eq!(row.canonical_row_id, coordinate);
    assert!(!row.is_placeholder());
    assert_eq!(row.content, "hydrated via delivered-ref demand alone");
    assert_eq!(row.created_at, 42);
}

#[test]
fn tags_match_requires_every_declared_tag_to_have_a_matching_value() {
    let match_tags = [("K".to_string(), BTreeSet::from(["30023".to_string()]))]
        .into_iter()
        .collect();
    let admitted = comment("c", "a", "d", 1, "x");
    assert!(tags_match(&match_tags, &admitted));

    let mut wrong_kind = admitted.clone();
    wrong_kind.tags = vec![vec!["K".to_string(), "1".to_string()]];
    assert!(!tags_match(&match_tags, &wrong_kind));
}

#[test]
fn demand_live_shape_and_admission_agree_on_the_same_demanded_target() {
    let demand = DeliveredRefDemand::new();
    demand.demand(nmp_feed::TypedRefTarget::Address {
        kind: KIND_ARTICLE,
        pubkey: "bob".to_string(),
        d: "d1".to_string(),
    });
    let admit = demand_admission(&demand, vec![KIND_ARTICLE]);
    let shape = demand_live_shape(&demand, vec![KIND_ARTICLE])().expect("shape");
    assert_eq!(shape.addresses.len(), 1);
    assert!(admit(&article("bob", "d1", 1, "x", "")));
    assert!(!admit(&article("bob", "other-d", 1, "y", "")));
}
