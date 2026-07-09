//! #3086/#3090 — invariant 1 (order-independence) of the composite feed's
//! `Delivered`-ref demand mechanism, end-to-end through a REAL
//! `NmpApp`/`FeedSessionHost` and a real (fixture) relay.
//!
//! Split out of `composite_feed_driving_example.rs` (the #3082 three-lane
//! dedupe driving example) for file-size discipline; shares its
//! `composite_feed_common/fixtures.rs` helpers.
//!
//! `composite_feed_delivered_ref_hydration_is_order_independent` — invariant
//! 1: the target (article) is covered by NO lane's own acquisition; the
//! ONLY path that can ever fetch it is a comment lane's `Delivered`-ref
//! demand driving a fresh dependent interest (`flat_session.rs`'s `apply`
//! re-syncing the Trellis `acquisition_adapter`, not just the
//! observed-projection `engine_observer`; see #3086/#3090). This test
//! drives that mechanism through BOTH possible delivery orders — the
//! article landing on the relay before the demanding comment ever arrives
//! ("target-first"), and the comment arriving first with the article only
//! published once the fresh post-demand subscription is already open
//! ("demand-first") — and proves the composite feed converges to the
//! IDENTICAL hydrated row (same `created_at`, content, author, and
//! accumulated provenance) in both cases: membership/order is a pure
//! function of the delivered set, never of wall-clock delivery order.

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
use nostr::Keys;
use support::*;

/// Which side of the `Delivered`-ref demand race the target article lands on
/// relative to the demanding comment, for
/// [`composite_feed_delivered_ref_hydration_is_order_independent`].
#[derive(Clone, Copy, Debug)]
enum DeliveryOrder {
    /// The article is published on the relay (and lands in `events`'
    /// backlog) BEFORE the comment ever arrives — i.e. before ANY
    /// subscription for the article's kind/coordinate exists. It can only
    /// reach this session once the demand-driven fresh dependent interest's
    /// own initial backfill replays it.
    TargetFirst,
    /// The comment arrives first (registering demand and opening the fresh
    /// dependent interest), and the article is only published on the relay
    /// afterward, over that already-open subscription.
    DemandFirst,
}

/// The subset of a hydrated [`nmp_feed::FeedRow`] this test compares across
/// delivery orders. `relay_provenance` is deliberately excluded: each
/// [`DeliveryOrder`] run spins up its OWN fixture relay (a distinct
/// `ws://127.0.0.1:<port>`), so that field differs by construction even
/// though every protocol-level fact converges.
#[derive(Debug, PartialEq)]
struct HydratedRowFacts {
    canonical_row_id: String,
    author_pubkey: String,
    content: String,
    created_at: u64,
    context: std::collections::BTreeSet<FeedRowContext>,
}

/// Drives the #3086/#3090 `Delivered`-ref demand mechanism end-to-end for one
/// [`DeliveryOrder`]: a single `nip22.root` comment lane is the ONLY lane (no
/// direct/authored lane admits the article's own author), so the article can
/// ONLY ever reach this composite session via the demand-driven fresh
/// dependent interest that `flat_session.rs`'s `apply` opens once the
/// comment's `Delivered` ref registers. Returns the row's hydrated facts once
/// the placeholder resolves.
fn run_delivered_ref_hydration(
    order: DeliveryOrder,
    article_author: &Keys,
    commenter: &Keys,
    d_tag: &str,
    comment_created_at: u64,
    article_created_at: u64,
) -> HydratedRowFacts {
    let rx = install_update_signal();

    let article_pk = article_author.public_key().to_hex();
    let coordinate = format!("{KIND_LONG_FORM_ARTICLE}:{article_pk}:{d_tag}");
    let article = article_event(article_author, d_tag, article_created_at, "the real body");

    // TargetFirst seeds the relay's own event backlog with the article
    // BEFORE any subscription exists for it — exactly like an old article
    // resurfacing, published long before any comment ever demands it.
    let seed = match order {
        DeliveryOrder::TargetFirst => vec![article.clone()],
        DeliveryOrder::DemandFirst => Vec::new(),
    };
    let mut relay = RecordingRelay::spawn(seed);
    let app = new_reduced_source_app_before_start();
    let app_ref = unsafe { &*app };
    start_app(app);
    add_relay(app, relay.url());

    let key = "test.3086.delivered-ref-order-independence";
    let params = CompositeFeedParams {
        key: ProjectionKey::app_owned(key).unwrap(),
        lanes: vec![FeedLane {
            source: FeedScope::Authors {
                authors: [commenter.public_key().to_hex()].into_iter().collect(),
            },
            match_kinds: vec![u32::from(KIND_COMMENT)],
            match_tags: BTreeMap::from([(
                TagKey("K".to_string()),
                [KIND_LONG_FORM_ARTICLE.to_string()].into_iter().collect(),
            )]),
            mapping: LaneMappingId(nmp_nip22::NIP22_ROOT_MAPPING_ID.to_string()),
        }],
        render_target_kinds: vec![KIND_LONG_FORM_ARTICLE],
        sort: SortPolicy::ByTargetCreatedAt,
        window: FeedWindowPolicy::bounded(80),
        item_projection: FeedItemProjection::FeedRows,
    };

    let registry = test_registry();
    let handle = app_ref
        .open_composite_feed_with_mappings_for_test(&params, &registry)
        .expect("composite feed opens through the real composition root");

    relay.wait_req("commenter kind:1111 (initial acquisition)", |f| {
        has_kind(f, u64::from(KIND_COMMENT))
    });

    relay.push_event(comment_event(commenter, &article_pk, d_tag, comment_created_at));
    wait_for(&rx, "comment placeholder row", || {
        flat_feed_ids(app_ref, key) == vec![coordinate.clone()]
    });
    assert!(
        flat_feed_cards(app_ref, key)[0].is_placeholder(),
        "the article has not been fetched yet ({order:?})"
    );

    // The fix under test: registering demand must drive a FRESH acquisition
    // resync, which opens a NEW dependent interest — observed here as a new
    // REQ naming kind:30023 (the article's own kind + coordinate), landing
    // on the relay strictly AFTER the comment was pushed. This holds
    // regardless of whether the article's bytes are already sitting in the
    // relay's backlog (TargetFirst) or not yet published (DemandFirst): the
    // client must open the subscription either way to ever receive it.
    let req = relay.wait_req("post-demand kind:30023 fetch (the fix)", |f| {
        has_kind(f, u64::from(KIND_LONG_FORM_ARTICLE))
    });
    assert!(
        req.filter.to_string().contains(&coordinate),
        "the fresh interest names the article's OWN coordinate, not a broad kind-only scan ({order:?}): {}",
        req.filter
    );

    if matches!(order, DeliveryOrder::DemandFirst) {
        relay.push_event(article.clone());
    }

    // Wait for FULL convergence: not just "no longer a placeholder" but the
    // accumulated provenance set fully unioned (CommentedBy + Authored). A
    // weaker "not placeholder" predicate can observe a transient
    // intermediate wire-projection revision where `kind`/`content` have
    // already flipped but the same merge's `context` union has not yet
    // propagated through to this snapshot read — the underlying engine
    // merge (`FlatFeed::ingest`) is atomic, but the typed-projection
    // snapshot this test reads through `run_typed_snapshot_projections` is
    // eventually consistent with it, same as any other reactive read here.
    wait_for(&rx, "article hydrates the row with full provenance", || {
        flat_feed_cards(app_ref, key)
            .first()
            .is_some_and(|row| !row.is_placeholder() && row.context.len() == 2)
    });

    let rows = flat_feed_cards(app_ref, key);
    assert_eq!(rows.len(), 1, "one row, ({order:?})");
    let row = rows.into_iter().next().expect("row present");

    app_ref.close_feed(&handle);
    let _ = rx;
    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();

    HydratedRowFacts {
        canonical_row_id: row.canonical_row_id,
        author_pubkey: row.author_pubkey,
        content: row.content,
        created_at: row.created_at,
        context: row.context.into_iter().collect(),
    }
}

/// #3086/#3090 invariant 1 — membership/order is a pure function of the
/// delivered set, never of wall-clock delivery order. Drives the SAME
/// `Delivered`-ref demand scenario (article reachable ONLY via a comment
/// lane's demand-driven fresh dependent interest; see
/// `run_delivered_ref_hydration`) through both possible orderings and proves
/// they converge to the identical hydrated row.
#[test]
fn composite_feed_delivered_ref_hydration_is_order_independent() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());

    let article_author = keys_from_byte(221); // NOT covered by any lane's own acquisition.
    let commenter = keys_from_byte(222);
    let d_tag = "order-independence-article";
    let comment_created_at = 500;
    let article_created_at = 50; // long before the comment — "old article resurfaces".

    let target_first = run_delivered_ref_hydration(
        DeliveryOrder::TargetFirst,
        &article_author,
        &commenter,
        d_tag,
        comment_created_at,
        article_created_at,
    );
    let demand_first = run_delivered_ref_hydration(
        DeliveryOrder::DemandFirst,
        &article_author,
        &commenter,
        d_tag,
        comment_created_at,
        article_created_at,
    );

    assert_eq!(
        target_first, demand_first,
        "the composite feed converges to the IDENTICAL hydrated row regardless of \
         whether the target was delivered before or after the demand that fetches it"
    );
    assert_eq!(
        target_first.created_at, article_created_at,
        "ByTargetCreatedAt: the row's created_at is the article's OWN real publish time"
    );
    assert_eq!(target_first.content, "the real body");
    assert_eq!(target_first.author_pubkey, article_author.public_key().to_hex());
    assert_eq!(
        target_first.context,
        std::collections::BTreeSet::from([
            FeedRowContext::Authored,
            FeedRowContext::CommentedBy {
                author_pubkey: commenter.public_key().to_hex(),
                comment_event_id: comment_event(
                    &commenter,
                    &article_author.public_key().to_hex(),
                    d_tag,
                    comment_created_at
                )
                .id
                .to_hex(),
                comment_created_at,
            }
        ]),
        "provenance accumulates the comment lane's CommentedBy contribution PLUS the \
         demand-hydration mechanism's own Authored tag for the delivered render-target \
         row (composite_compiler.rs's build_composite_rows always tags the demanded \
         target's row Authored — there is no separate direct/authored lane here)"
    );
}
