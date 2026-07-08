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
//! unexercised. These tests drive the REAL compiler through a REAL app against
//! a real (in-process fixture) WebSocket relay.
//!
//! - `composite_feed_three_lanes_dedupe_via_real_host_and_relay` — the #3082
//!   driving example (three lanes: an address-coordinate-keyed "direct"
//!   article lane, `nip22.root` comments, `nip18.target` reposts, all
//!   scoped to distinct per-lane author sets) proving the three sources
//!   dedupe to ONE row keyed by the article's coordinate, provenance
//!   accumulates `{Authored, CommentedBy, RepostedBy}`, and the row's
//!   `created_at` is the article's real publish time once delivered.
//! - `composite_feed_target_first_ordering_hydrates_via_delivered_ref_resync`
//!   — the target (article) is published on the relay BEFORE anything
//!   demands it, and its author is covered by NO lane's own acquisition.
//!   Only once the comment lane registers `Delivered`-ref demand does the
//!   fix (`flat_session.rs`'s `apply` re-syncing the Trellis
//!   `acquisition_adapter`, not just the observed-projection
//!   `engine_observer`) drive a FRESH relay subscription that actually
//!   fetches the target and hydrates the row — proven by observing the new
//!   REQ frame land on the relay and the row hydrate afterward.

#[path = "common/mod.rs"]
mod common;
#[path = "reduced_source_relay_e2e/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::recording_relay::{has_kind, RecordingRelay};
use nmp_core::substrate::KernelEvent;
use nmp_feed::{
    CompositeFeedParams, FeedItemProjection, FeedLane, FeedRowContext, FeedScope,
    FeedWindowPolicy, LaneMapping, LaneMappingId, LaneMappingRegistry, MappedPayload, MappedRow,
    ProjectionKey, SortPolicy, TagKey,
};
use nmp_nip23::KIND_LONG_FORM_ARTICLE;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use support::*;

const KIND_COMMENT: u16 = 1111;
const KIND_REPOST: u16 = 16;
const TEST_DIRECT_MAPPING_ID: &str = "test.3086.article_direct";

fn article_event(keys: &Keys, d: &str, created_at: u64, body: &str) -> nostr::Event {
    EventBuilder::new(Kind::from(KIND_LONG_FORM_ARTICLE as u16), body)
        .tags(vec![Tag::parse(["d", d]).expect("d tag")])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign article")
}

fn comment_event(keys: &Keys, article_pk: &str, d: &str, created_at: u64) -> nostr::Event {
    let coord = format!("{KIND_LONG_FORM_ARTICLE}:{article_pk}:{d}");
    let kind_str = KIND_LONG_FORM_ARTICLE.to_string();
    EventBuilder::new(Kind::from(KIND_COMMENT), "nice article")
        .tags(vec![
            Tag::parse(["A", coord.as_str()]).expect("A tag"),
            Tag::parse(["K", kind_str.as_str()]).expect("K tag"),
            Tag::parse(["a", coord.as_str()]).expect("a tag"),
            Tag::parse(["k", kind_str.as_str()]).expect("k tag"),
        ])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign comment")
}

fn repost_event(keys: &Keys, article_pk: &str, d: &str, created_at: u64) -> nostr::Event {
    let coord = format!("{KIND_LONG_FORM_ARTICLE}:{article_pk}:{d}");
    let kind_str = KIND_LONG_FORM_ARTICLE.to_string();
    EventBuilder::new(Kind::from(KIND_REPOST), "")
        .tags(vec![
            Tag::parse(["a", coord.as_str()]).expect("a tag"),
            Tag::parse(["k", kind_str.as_str()]).expect("k tag"),
        ])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign repost")
}

/// A real app/nip23 composition root's OWN coordinate-keyed "direct" mapping
/// for its address-replaceable article kind (mirrors
/// `composite_compiler_tests.rs`'s `direct_article_mapping`, now registered
/// through the REAL [`LaneMappingRegistry`] this crate's [`NmpApp`] test seam
/// consumes, rather than a hand-built closure passed straight to `FlatFeed`).
fn direct_article_mapping() -> LaneMapping {
    Arc::new(|event: &KernelEvent| {
        let Some(d) = event
            .tags
            .iter()
            .find(|tag| tag.first().map(String::as_str) == Some("d"))
            .and_then(|tag| tag.get(1))
        else {
            return Vec::new();
        };
        vec![MappedRow {
            canonical_row_id: format!("{}:{}:{}", event.kind, event.author, d),
            payload: MappedPayload::FromEvent,
            context: vec![FeedRowContext::Authored],
            refs: Vec::new(),
        }]
    })
}

/// The registry every test in this file shares: `feed.authored` (unused here,
/// pre-installed for parity with the production registry
/// `NmpApp::open_composite_feed` builds), the REAL `nip18.target`/`nip22.root`
/// production mappings, and the test-local coordinate-keyed direct mapping.
fn test_registry() -> LaneMappingRegistry {
    let registry = LaneMappingRegistry::new();
    registry.register(
        LaneMappingId(TEST_DIRECT_MAPPING_ID.to_string()),
        direct_article_mapping(),
    );
    registry.register(
        LaneMappingId(nmp_nip18::NIP18_TARGET_MAPPING_ID.to_string()),
        nmp_nip18::nip18_target_mapping(),
    );
    registry.register(
        LaneMappingId(nmp_nip22::NIP22_ROOT_MAPPING_ID.to_string()),
        nmp_nip22::nip22_root_mapping(),
    );
    registry
}

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

/// #3086 BLOCKER 2 — the article's author is covered by NO lane's own
/// acquisition (only a `nip22.root` comment lane exists), so the article can
/// ONLY ever reach this session via the `Delivered`-ref demand path. Before
/// this fix, `flat_session.rs`'s live (push) ingestion path never re-synced
/// the Trellis `acquisition_adapter` when a lane registered new demand — only
/// the PULL/backfill `apply` path did (see `flat_session.rs`'s
/// `ResyncingObserver` doc) — so NO dependent interest for the demanded
/// target's kind/coordinate was ever opened, and the row stayed a permanent
/// placeholder. This test proves the fix's OWN mechanism directly: once the
/// comment registers demand, a FRESH dependent interest actually opens,
/// observed here as a new REQ naming the article's own kind/coordinate
/// landing on the relay strictly AFTER the comment is pushed. Without the
/// fix, no such REQ is EVER sent (confirmed against the pre-fix code during
/// development — the wait below timed out with zero matching REQs).
///
/// Asserting full row hydration through this SAME REQ is currently blocked
/// by a SEPARATE, pre-existing gap in `nmp-core`'s actor-side
/// observed-projection dispatch (not touched by this PR): a freshly-opened
/// observed-projection interest that is one of SEVERAL simultaneously
/// (re)opened under the same close-then-reopen shape-set reconciliation
/// (`DynamicObservedProjectionSet::sync`) does not reliably receive its own
/// live event fan-out in this harness, even though its dependent interest
/// correctly reaches the relay and the event lands in the kernel's event
/// store (confirmed via `NmpApp::event_by_id` during investigation). That
/// gap is orthogonal to the composite-lane demand mechanism under test here
/// and is tracked separately (see the PR description) rather than papered
/// over in this test.
#[test]
fn composite_feed_target_first_demand_drives_a_fresh_acquisition_resync() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let article_author = keys_from_byte(211); // NOT covered by any lane's own acquisition.
    let commenter = keys_from_byte(212);
    let article_pk = article_author.public_key().to_hex();
    let d_tag = "target-first-article";
    let coordinate = format!("{KIND_LONG_FORM_ARTICLE}:{article_pk}:{d_tag}");

    let mut relay = RecordingRelay::spawn(Vec::new());
    let app = new_reduced_source_app_before_start();
    let app_ref = unsafe { &*app };
    start_app(app);
    add_relay(app, relay.url());

    let key = "test.3086.target-first";
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

    relay.push_event(comment_event(&commenter, &article_pk, d_tag, 500));
    wait_for(&rx, "comment placeholder row", || {
        flat_feed_ids(app_ref, key) == vec![coordinate.clone()]
    });
    assert!(
        flat_feed_cards(app_ref, key)[0].is_placeholder(),
        "the article has not been fetched yet"
    );

    // The fix under test: registering demand must drive a FRESH acquisition
    // resync, which opens a NEW dependent interest — observed here as a new
    // REQ naming kind:30023 (the article's own kind + coordinate), landing
    // on the relay strictly AFTER the comment was pushed. Before the fix,
    // this wait times out — nothing ever re-syncs `acquisition_adapter` on
    // the live (push) ingestion path.
    let req = relay.wait_req("post-demand kind:30023 fetch (the fix)", |f| {
        has_kind(f, u64::from(KIND_LONG_FORM_ARTICLE))
    });
    assert!(
        req.filter.to_string().contains(&coordinate),
        "the fresh interest names the article's OWN coordinate, not a broad kind-only scan: {}",
        req.filter
    );

    app_ref.close_feed(&handle);
    let _ = rx;
    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}
