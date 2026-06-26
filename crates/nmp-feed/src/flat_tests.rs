use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;

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
            Some(sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            ))
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
fn equal_timestamp_sources_keep_deterministic_first_source() {
    let feed = FlatFeed::new(
        Arc::new(|_| true),
        Arc::new(|event| {
            Some(sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            ))
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
            Some(sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            ))
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
            Some(sourced_item(
                &event.id.replace("-wrapper", ""),
                &event.id,
                event.created_at,
                &event.content,
            ))
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
        Arc::new(|event| Some(item(&event.id, event.created_at, &event.content))),
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
            Some(sourced_item(
                "target",
                &event.id,
                event.created_at,
                &event.content,
            ))
        }),
        None,
        merge,
    );

    feed.on_kernel_event(&event("wrapper", 6, 20, "repost"));
    feed.on_kernel_event(&event("target", 1, 10, "original"));

    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(snap.cards[0].card, "original+repost");
    assert_eq!(snap.cards[0].attribution, Vec::<()>::new());
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
                .and_then(|raw| raw.parse::<u64>().ok())?;
            Some(item(&event.id, rank, &event.content))
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
