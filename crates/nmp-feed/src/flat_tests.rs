use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;

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
        sort_created_at,
        card: card.to_string(),
    }
}

#[test]
fn canonical_identity_dedups_and_keeps_newer_sort_source() {
    let feed = FlatFeed::new(
        Arc::new(|_| true),
        Arc::new(|event| Some(item("target", event.created_at, &event.content))),
    );

    feed.on_kernel_event(&event("target", 1, 10, "original"));
    feed.on_kernel_event(&event("wrapper", 6, 20, "repost"));

    let snap = feed.snapshot(&FeedRequest::default());
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card, "repost");
    assert_eq!(snap.page.unwrap().total_blocks, 1);
}

#[test]
fn custom_merge_can_hydrate_existing_bumped_item() {
    let merge: FlatFeedMerge<String> = Arc::new(|existing, incoming| {
        if let Some(existing) = existing {
            if existing.sort_created_at > incoming.sort_created_at {
                return FlatFeedItem {
                    id: existing.id.clone(),
                    sort_created_at: existing.sort_created_at,
                    card: format!("{}+{}", incoming.card, existing.card),
                };
            }
        }
        incoming
    });
    let feed = FlatFeed::with_merge(
        Arc::new(|_| true),
        Arc::new(|event| Some(item("target", event.created_at, &event.content))),
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
