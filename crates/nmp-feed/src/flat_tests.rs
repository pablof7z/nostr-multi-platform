use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;

use crate::{FeedWindowResetPolicy, DEFAULT_FEED_WINDOW_LIMIT};

use super::*;

fn event(id: &str, kind: u32, created_at: u64, content: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: "author".to_string(),
        kind,
        created_at,
        tags: Vec::new(),
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

fn ranked_event(id: &str, created_at: u64, rank: u64, content: &str) -> KernelEvent {
    let mut event = event(id, 30_023, created_at, content);
    event.tags.push(vec!["rank".to_string(), rank.to_string()]);
    event
}

fn item(id: &str, sort_created_at: u64, card: &str) -> FlatFeedItem<String> {
    FlatFeedItem {
        id: id.to_string(),
        source_id: id.to_string(),
        sort_created_at,
        card: card.to_string(),
    }
}

fn sourced_item(
    id: &str,
    source_id: &str,
    sort_created_at: u64,
    card: &str,
) -> FlatFeedItem<String> {
    FlatFeedItem {
        id: id.to_string(),
        source_id: source_id.to_string(),
        sort_created_at,
        card: card.to_string(),
    }
}

#[test]
fn canonical_identity_dedups_and_keeps_newer_sort_source() {
    let feed = FlatFeed::new(
        Arc::new(|_| true),
        Arc::new(|event| {
            vec![sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            )]
        }),
    );

    feed.on_kernel_event(&event("target", 1, 10, "original"));
    feed.on_kernel_event(&event("wrapper", 6, 20, "repost"));

    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card, "repost");
    assert_eq!(snap.page.unwrap().total_blocks, 1);
}

#[test]
fn reversed_arrival_order_wrapper_before_target_still_surfaces_the_row() {
    // #3099 Bug B (the `RootIndexedFeed`/`pending_attributions` cache-luck
    // class the `FlatFeed` engine replaced — see
    // `docs/perf/composite-feed-architecture.md` §2 and this module's own
    // docs). The old engine buffered a reply/repost's contribution under the
    // WRAPPER's own id in a side table that only ever drained by the TARGET
    // id once a root arrived — so a wrapper/reply arriving before its target
    // was silently orphaned and never surfaced. `FlatFeed` has no side
    // table: every source maps directly onto the same canonical `row_id`
    // via `st.rows`, so ingest order cannot matter. Prove the reversed
    // order (wrapper FIRST, target SECOND) surfaces identically to the
    // target-first case already proven by
    // `canonical_identity_dedups_and_keeps_newer_sort_source`.
    let feed = FlatFeed::new(
        Arc::new(|_| true),
        Arc::new(|event| {
            vec![sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            )]
        }),
    );

    // The wrapper arrives BEFORE the target it points at is ever ingested.
    feed.on_kernel_event(&event("wrapper", 6, 20, "repost"));
    let mid_snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(
        mid_snap.cards.len(),
        1,
        "the wrapper's contribution must surface immediately — never buffered \
         under an id nothing ever drains"
    );
    assert_eq!(mid_snap.cards[0].card, "repost");

    // The target arrives later.
    feed.on_kernel_event(&event("target", 1, 10, "original"));
    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(snap.cards.len(), 1, "still ONE row — same canonical id");
    assert_eq!(
        snap.cards[0].card, "repost",
        "merge outcome must match the target-first ordering — the newer \
         sort_created_at source wins regardless of arrival order"
    );
    assert_eq!(snap.page.unwrap().total_blocks, 1);
}

#[test]
fn equal_timestamp_sources_keep_deterministic_first_source() {
    let feed = FlatFeed::new(
        Arc::new(|_| true),
        Arc::new(|event| {
            vec![sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            )]
        }),
    );

    feed.on_kernel_event(&event("aaa", 6, 20, "lower source"));
    feed.on_kernel_event(&event("zzz", 6, 20, "higher source"));

    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card, "higher source");
}

#[test]
fn removing_one_source_recomputes_canonical_row() {
    let feed = FlatFeed::new(
        Arc::new(|_| true),
        Arc::new(|event| {
            vec![sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            )]
        }),
    );

    feed.on_kernel_event(&event("target", 1, 10, "original"));
    feed.on_kernel_event(&event("wrapper", 6, 20, "repost"));
    assert_eq!(
        feed.snapshot(&FeedRequest::default()).cards[0].card,
        "repost"
    );

    assert!(feed.remove_source("target", "wrapper"));
    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card, "original");
}

#[test]
fn removing_matching_sources_drops_empty_rows_only() {
    let feed = FlatFeed::new(
        Arc::new(|_| true),
        Arc::new(|event| {
            vec![sourced_item(
                &event.id.replace("-wrapper", ""),
                &event.id,
                event.created_at,
                &event.content,
            )]
        }),
    );

    feed.on_kernel_event(&event("one", 1, 10, "one target"));
    feed.on_kernel_event(&event("one-wrapper", 6, 20, "one repost"));
    feed.on_kernel_event(&event("two-wrapper", 6, 30, "two repost"));

    let removed = feed.remove_sources_if(|item| item.source_id.ends_with("-wrapper"));
    assert_eq!(removed, 2);

    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card, "one target");
}

#[test]
fn perspective_reset_clears_rows_and_restores_first_window() {
    let feed = FlatFeed::new(
        Arc::new(|_| true),
        Arc::new(|event| vec![item(&event.id, event.created_at, &event.content)]),
    );

    let total_rows = DEFAULT_FEED_WINDOW_LIMIT + 5;
    for idx in 0..total_rows {
        feed.on_kernel_event(&event(&format!("event-{idx:02}"), 1, idx as u64, "row"));
    }
    assert_eq!(
        feed.snapshot_current_window().cards.len(),
        DEFAULT_FEED_WINDOW_LIMIT
    );
    assert!(feed.grow_visible_window());
    assert_eq!(feed.snapshot_current_window().cards.len(), total_rows);

    assert!(feed.reset_for_perspective_change());
    let snap = feed.snapshot_current_window();
    assert!(snap.cards.is_empty());
    assert_eq!(snap.page.unwrap().limit, DEFAULT_FEED_WINDOW_LIMIT);
    assert!(!feed.reset_for_perspective_change());
}

#[test]
fn flat_feed_window_policy_drives_initial_grow_and_reset_limits() {
    let policy = FeedWindowPolicy {
        initial_limit: 5,
        page_size: 3,
        max_visible: 9,
        reset: FeedWindowResetPolicy::ResetToInitial,
        ..FeedWindowPolicy::default()
    };
    let feed = FlatFeed::with_merge_and_window_policy(
        Arc::new(|_| true),
        Arc::new(|event| vec![item(&event.id, event.created_at, &event.content)]),
        None,
        default_merge(),
        policy,
    );

    for idx in 0..12 {
        feed.on_kernel_event(&event(&format!("event-{idx:02}"), 1, idx as u64, "row"));
    }

    assert_eq!(feed.snapshot_current_window().cards.len(), 5);
    assert!(feed.grow_visible_window());
    assert_eq!(feed.snapshot_current_window().cards.len(), 8);
    assert!(feed.grow_visible_window());
    assert_eq!(
        feed.snapshot_current_window().cards.len(),
        9,
        "max_visible caps regrow"
    );
    assert!(!feed.grow_visible_window());

    assert!(feed.reset_for_perspective_change());
    for idx in 0..12 {
        feed.on_kernel_event(&event(
            &format!("after-reset-{idx:02}"),
            1,
            idx as u64,
            "row",
        ));
    }
    assert_eq!(feed.snapshot_current_window().cards.len(), 5);
}

#[test]
fn custom_merge_can_hydrate_existing_bumped_item() {
    let merge: FlatFeedMerge<String> = Arc::new(|existing, incoming| {
        if let Some(existing) = existing {
            if existing.sort_created_at > incoming.sort_created_at {
                return FlatFeedItem {
                    id: existing.id.clone(),
                    source_id: existing.source_id.clone(),
                    sort_created_at: existing.sort_created_at,
                    card: format!("{}+{}", incoming.card, existing.card),
                };
            }
        }
        incoming
    });
    let feed = FlatFeed::with_merge(
        Arc::new(|_| true),
        Arc::new(|event| {
            vec![sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            )]
        }),
        None,
        merge,
    );

    feed.on_kernel_event(&event("wrapper", 6, 20, "repost"));
    feed.on_kernel_event(&event("target", 1, 10, "original"));

    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(snap.cards[0].card, "original+repost");
}

#[test]
fn caller_supplied_admission_and_rank_key_drive_flat_feed() {
    let feed = FlatFeed::new(
        Arc::new(|event| event.content.split_whitespace().count() >= 4),
        Arc::new(|event| {
            let rank = event
                .tags
                .iter()
                .find(|tag| tag.first().is_some_and(|key| key == "rank"))
                .and_then(|tag| tag.get(1))
                .and_then(|raw| raw.parse::<u64>().ok());
            rank.map(|rank| item(&event.id, rank, &event.content))
                .into_iter()
                .collect()
        }),
    );

    feed.on_kernel_event(&ranked_event("short-high", 100, 1_000, "too short"));
    feed.on_kernel_event(&ranked_event(
        "old-high",
        10,
        900,
        "high quality article with old timestamp",
    ));
    feed.on_kernel_event(&ranked_event(
        "new-low",
        100,
        100,
        "acceptable article with newer timestamp",
    ));

    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(
        snap.cards
            .iter()
            .map(|row| row.card.as_str())
            .collect::<Vec<_>>(),
        vec![
            "high quality article with old timestamp",
            "acceptable article with newer timestamp"
        ],
        "admission and ordering are owned by caller-supplied closures"
    );
}

#[path = "flat_policy_tests.rs"]
mod policy_tests;
