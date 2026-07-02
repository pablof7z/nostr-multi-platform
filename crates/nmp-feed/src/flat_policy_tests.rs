use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_planner::InterestShape;

use super::super::*;

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

#[test]
fn same_interest_shape_custom_policies_are_independent() {
    let interest = InterestShape {
        authors: ["a".repeat(64)].into_iter().collect(),
        kinds: [30_023u32].into_iter().collect(),
        ..Default::default()
    };
    let quality_feed = FlatFeed::with_interest(
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
        Some(interest.clone()),
    );
    let photo_feed = FlatFeed::with_interest(
        Arc::new(|event| {
            event
                .tags
                .iter()
                .any(|tag| tag.first().is_some_and(|key| key == "photo"))
        }),
        Arc::new(|event| Some(item(&event.id, event.created_at, &event.content))),
        Some(interest.clone()),
    );

    for event in [
        ranked_event("short-high", 30, 1_000, "too short"),
        ranked_event(
            "article-high",
            10,
            900,
            "longform article with strong local score",
        ),
        {
            let mut event = ranked_event("photo-low", 40, 100, "photo row");
            event.tags.push(vec!["photo".to_string()]);
            event
        },
    ] {
        quality_feed.on_kernel_event(&event);
        photo_feed.on_kernel_event(&event);
    }

    assert_eq!(quality_feed.interest_shape(), Some(interest.clone()));
    assert_eq!(photo_feed.interest_shape(), Some(interest));
    assert_eq!(
        quality_feed
            .snapshot(&FeedRequest::default())
            .cards
            .iter()
            .map(|row| row.card.as_str())
            .collect::<Vec<_>>(),
        vec!["longform article with strong local score"],
        "same acquisition shape must not force a shared admission/order policy"
    );
    assert_eq!(
        photo_feed
            .snapshot(&FeedRequest::default())
            .cards
            .iter()
            .map(|row| row.card.as_str())
            .collect::<Vec<_>>(),
        vec!["photo row"],
        "sibling feed keeps its own predicate and ordering"
    );
}

#[test]
fn resetting_one_same_shape_feed_does_not_clear_sibling_policy_state() {
    let interest = InterestShape {
        authors: ["b".repeat(64)].into_iter().collect(),
        kinds: [1u32].into_iter().collect(),
        ..Default::default()
    };
    let even_feed = FlatFeed::with_interest(
        Arc::new(|event| event.created_at % 2 == 0),
        Arc::new(|event| Some(item(&event.id, event.created_at, &event.content))),
        Some(interest.clone()),
    );
    let odd_feed = FlatFeed::with_interest(
        Arc::new(|event| event.created_at % 2 == 1),
        Arc::new(|event| Some(item(&event.id, event.created_at, &event.content))),
        Some(interest),
    );

    for event in [
        event("even-old", 1, 2, "even old"),
        event("odd-old", 1, 3, "odd old"),
    ] {
        even_feed.on_kernel_event(&event);
        odd_feed.on_kernel_event(&event);
    }
    assert_eq!(even_feed.len(), 1);
    assert_eq!(odd_feed.len(), 1);

    assert!(even_feed.reset_for_perspective_change());
    assert!(even_feed.is_empty());
    assert_eq!(
        odd_feed.snapshot(&FeedRequest::default()).cards[0].card,
        "odd old",
        "resetting one feed instance must not clear another feed with the same InterestShape"
    );

    for event in [
        event("even-new", 1, 4, "even new"),
        event("odd-new", 1, 5, "odd new"),
    ] {
        even_feed.on_kernel_event(&event);
        odd_feed.on_kernel_event(&event);
    }

    assert_eq!(
        even_feed
            .snapshot(&FeedRequest::default())
            .cards
            .iter()
            .map(|row| row.card.as_str())
            .collect::<Vec<_>>(),
        vec!["even new"],
        "reset feed regrows only from rows admitted under its own policy"
    );
    assert_eq!(
        odd_feed
            .snapshot(&FeedRequest::default())
            .cards
            .iter()
            .map(|row| row.card.as_str())
            .collect::<Vec<_>>(),
        vec!["odd new", "odd old"],
        "sibling feed retains and continues its independent policy state"
    );
}
