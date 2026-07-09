//! #3082/#3086 — `open_composite_feed` end-to-end through a REAL
//! `NmpApp`/`FeedSessionHost` and a real (fixture) relay.
//!
//! `composite_compiler_tests.rs` (in `nmp-feed-session`) proves the row-building
//! + merge + demand LOGIC by hand-building a `FlatFeed` directly — it
//! deliberately bypasses `FeedSessionHost`/acquisition-resolution. This file
//! is the gap that leaves open: `open_composite_feed` itself had ZERO callers
//! before this PR, so registry lookup, `fold_lanes`, `revoke_all`,
//! `build_flat_scope_session` integration, and LIVE acquisition (actually
//! opening relay subscriptions and receiving delivered events) were entirely
//! unexercised. This test drives the REAL compiler through a REAL app against
//! a real (in-process fixture) WebSocket relay.
//!
//! `composite_feed_three_lanes_dedupe_via_real_host_and_relay` — the #3082
//! driving example (three lanes: an address-coordinate-keyed "direct"
//! article lane, `nip22.root` comments, `nip18.target` reposts, all
//! scoped to distinct per-lane author sets) proving the three sources
//! dedupe to ONE row keyed by the article's coordinate, provenance
//! accumulates `{Authored, CommentedBy, RepostedBy}`, and the row's
//! `created_at` is the article's real publish time once delivered.
//!
//! The sibling `composite_feed_delivered_ref_order_independence.rs` proves
//! invariant 1 (order-independence) of the `Delivered`-ref demand mechanism
//! (#3086/#3090) — split out to its own file for file-size discipline.

#[path = "common/mod.rs"]
mod common;
#[path = "reduced_source_relay_e2e/support.rs"]
mod support;
#[path = "composite_feed_common/fixtures.rs"]
mod composite_fixtures;

use std::collections::BTreeMap;

use common::recording_relay::{has_kind, RecordingRelay};
use composite_fixtures::*;
use nmp_feed::{
    CompositeFeedParams, FeedItemProjection, FeedLane, FeedRowContext, FeedScope,
    FeedWindowPolicy, LaneMappingId, ProjectionKey, SortPolicy, TagKey,
};
use nmp_nip23::KIND_LONG_FORM_ARTICLE;
use support::*;

/// #3082 driving example, over a REAL host + relay (not a hand-built
/// `FlatFeed`): three lanes — an authored/direct article lane, a `nip22.root`
/// comment lane, a `nip18.target` repost lane — all scoped to DISTINCT
/// per-lane author sets, deduping to ONE row keyed by the article's own
/// coordinate.
#[test]
fn composite_feed_three_lanes_dedupe_via_real_host_and_relay() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let article_author = keys_from_byte(201);
    let commenter = keys_from_byte(202);
    let reposter = keys_from_byte(203);
    let article_pk = article_author.public_key().to_hex();
    let commenter_pk = commenter.public_key().to_hex();
    let reposter_pk = reposter.public_key().to_hex();
    let d_tag = "my-article";
    let coordinate = format!("{KIND_LONG_FORM_ARTICLE}:{article_pk}:{d_tag}");

    let mut relay = RecordingRelay::spawn(Vec::new());
    let app = new_reduced_source_app_before_start();
    let app_ref = unsafe { &*app };
    start_app(app);
    add_relay(app, relay.url());

    let key = "test.3086.driving-example";
    let params = CompositeFeedParams {
        key: ProjectionKey::app_owned(key).unwrap(),
        lanes: vec![
            FeedLane {
                source: FeedScope::Authors {
                    authors: [article_pk.clone()].into_iter().collect(),
                },
                match_kinds: vec![KIND_LONG_FORM_ARTICLE],
                match_tags: BTreeMap::new(),
                mapping: LaneMappingId(TEST_DIRECT_MAPPING_ID.to_string()),
            },
            FeedLane {
                source: FeedScope::Authors {
                    authors: [commenter_pk.clone()].into_iter().collect(),
                },
                match_kinds: vec![u32::from(KIND_COMMENT)],
                match_tags: BTreeMap::from([(
                    TagKey("K".to_string()),
                    [KIND_LONG_FORM_ARTICLE.to_string()].into_iter().collect(),
                )]),
                mapping: LaneMappingId(nmp_nip22::NIP22_ROOT_MAPPING_ID.to_string()),
            },
            FeedLane {
                source: FeedScope::Authors {
                    authors: [reposter_pk.clone()].into_iter().collect(),
                },
                match_kinds: vec![u32::from(KIND_REPOST)],
                match_tags: BTreeMap::from([(
                    TagKey("k".to_string()),
                    [KIND_LONG_FORM_ARTICLE.to_string()].into_iter().collect(),
                )]),
                mapping: LaneMappingId(nmp_nip18::NIP18_TARGET_MAPPING_ID.to_string()),
            },
        ],
        render_target_kinds: vec![KIND_LONG_FORM_ARTICLE],
        sort: SortPolicy::ByTargetCreatedAt,
        window: FeedWindowPolicy::bounded(80),
        item_projection: FeedItemProjection::FeedRows,
    };

    let registry = test_registry();
    let handle = app_ref
        .open_composite_feed_with_mappings_for_test(&params, &registry)
        .expect("composite feed opens through the real composition root");

    // Each of the three lanes resolved its OWN acquisition through the SAME
    // step-3 compiler `open_feed` uses (`fold_lanes` folded all three into one
    // combined acquisition set) — proven by three DISTINCT REQ frames landing
    // on the relay.
    relay.wait_req("article-author kind:30023", |f| {
        has_kind(f, u64::from(KIND_LONG_FORM_ARTICLE))
    });
    relay.wait_req("commenter kind:1111", |f| has_kind(f, u64::from(KIND_COMMENT)));
    relay.wait_req("reposter kind:16", |f| has_kind(f, u64::from(KIND_REPOST)));

    // Comment and repost arrive first — before the article itself. Each
    // lane's `Delivered` ref registers demand with this session's OWN
    // `DeliveredRefDemand`.
    relay.push_event(comment_event(&commenter, &article_pk, d_tag, 200));
    wait_for(&rx, "comment placeholder row", || {
        flat_feed_ids(app_ref, key) == vec![coordinate.clone()]
    });
    let placeholder = flat_feed_cards(app_ref, key);
    assert_eq!(placeholder.len(), 1);
    assert!(placeholder[0].is_placeholder(), "no lane delivered the article yet");

    relay.push_event(repost_event(&reposter, &article_pk, d_tag, 210));
    wait_for(&rx, "comment + repost provenance", || {
        flat_feed_cards(app_ref, key)
            .first()
            .is_some_and(|row| row.context.len() == 2)
    });

    // The article itself, published LONG before either interaction (a
    // realistic "old article resurfaced" timeline) — arrives via the direct
    // lane's OWN already-open subscription (its author is admitted directly).
    let real_created_at = 50;
    relay.push_event(article_event(&article_author, d_tag, real_created_at, "the real body"));
    wait_for(&rx, "article hydrates the row", || {
        flat_feed_cards(app_ref, key)
            .first()
            .is_some_and(|row| !row.is_placeholder())
    });

    let rows = flat_feed_cards(app_ref, key);
    assert_eq!(rows.len(), 1, "three sources dedupe to ONE row");
    let row = &rows[0];
    assert_eq!(row.canonical_row_id, coordinate);
    assert_eq!(row.content, "the real body");
    assert_eq!(row.author_pubkey, article_pk);
    assert_eq!(
        row.created_at, real_created_at,
        "ByTargetCreatedAt: the row's created_at is the article's OWN real \
         publish time, not the comment/repost interaction time"
    );
    let contexts: std::collections::BTreeSet<_> = row.context.iter().cloned().collect();
    assert_eq!(
        contexts,
        std::collections::BTreeSet::from([
            FeedRowContext::Authored,
            FeedRowContext::CommentedBy {
                author_pubkey: commenter_pk,
                comment_event_id: comment_event(&commenter, &article_pk, d_tag, 200)
                    .id
                    .to_hex(),
                comment_created_at: 200,
            },
            FeedRowContext::RepostedBy {
                author_pubkey: reposter_pk,
                note_created_at: 210,
            },
        ]),
        "provenance accumulates Authored + CommentedBy + RepostedBy"
    );

    app_ref.close_feed(&handle);
    let _ = rx;
    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}
