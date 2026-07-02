//! Basic save/query behavior: single-shot save, duplicate rejection,
//! filter-based querying (author/kind/time-range), and the full expected
//! result set after NIP-09 deletion validation runs over the fixture corpus.

use std::time::Duration;

use async_utility::time;

use crate::*;

use super::fixtures::{decode_events, setup_db, TempDatabase, EVENTS};

#[tokio::test]
async fn test_save_and_query() {
    let (db, _temp_dir) = setup_db().await;
    let events = decode_events();

    // Save all events (some will be rejected due to invalid deletions)
    for (i, event) in events.iter().enumerate() {
        let status = db.save_event(event).await.expect("Failed to save event");
        if i == 7 || i == 11 {
            // These should be rejected for invalid deletions
            assert!(!status.is_success());
        } else {
            assert!(matches!(status, SaveEventStatus::Success));
        }

        // NOTE: Sleep prevents automatic batching - events in the same batch share
        // a database snapshot and can't see each other's changes. Deletion events
        // (7,11) must "see" target events, and replaceable events must observe
        // earlier events to replace them. Sleep forces sequential processing.
        // Use this pattern when event N must observe changes from event N-1.
        time::sleep(Duration::from_millis(10)).await;
    }

    // Query all events
    let saved_events = db.query(Filter::new()).await.expect("Failed to query");
    // Expected: 8 events after applying coordinate deletion validation
    assert_eq!(saved_events.len(), 8);
}

#[tokio::test]
async fn test_save_duplicate() {
    let (db, _temp_dir) = setup_db().await;
    let events = decode_events();
    let event = &events[0];

    // Save event first time
    let status = db.save_event(event).await.expect("Failed to save event");
    assert!(matches!(status, SaveEventStatus::Success));

    // Try to save again
    let status = db.save_event(event).await.expect("Failed to save event");
    assert!(matches!(
        status,
        SaveEventStatus::Rejected(nostr_database::RejectedReason::Duplicate)
    ));
}

#[tokio::test]
async fn test_query_by_filter() {
    let (db, _temp_dir) = setup_db().await;
    let events = decode_events();

    // Save all events
    for event in &events {
        db.save_event(event).await.expect("Failed to save event");
    }

    // Query by author
    let author_filter = Filter::new().author(events[0].pubkey);
    let author_events = db.query(author_filter).await.expect("Failed to query");
    assert!(!author_events.is_empty());
    assert!(author_events.iter().all(|e| e.pubkey == events[0].pubkey));

    // Query by kind
    let kind_filter = Filter::new().kind(Kind::TextNote);
    let kind_events = db.query(kind_filter).await.expect("Failed to query");
    assert!(!kind_events.is_empty());
    assert!(kind_events.iter().all(|e| e.kind == Kind::TextNote));

    // Query by time range
    let since = Timestamp::from_secs(1704644590);
    let until = Timestamp::from_secs(1704644620);
    let time_filter = Filter::new().since(since).until(until);
    let time_events = db.query(time_filter).await.expect("Failed to query");
    assert!(!time_events.is_empty());
    assert!(time_events
        .iter()
        .all(|e| e.created_at >= since && e.created_at <= until));
}

#[tokio::test]
async fn test_expected_query_result() {
    let db = TempDatabase::new();

    // Save events individually to avoid batching issues during test
    for (i, event_str) in EVENTS.into_iter().enumerate() {
        let event = Event::from_json(event_str).unwrap();
        let status = db.save_event(&event).await.unwrap();

        // Invalid deletions (Event 7 and 11) should be rejected
        if i == 7 || i == 11 {
            assert!(!status.is_success(), "Event {} should be rejected", i);
        }

        // Add a small delay to ensure each event is processed individually
        time::sleep(Duration::from_millis(10)).await;
    }

    // Expected output after applying NIP-09 deletion validation
    // Events 7 and 11 are rejected for invalid deletion attempts
    let expected_output = vec![
        Event::from_json(EVENTS[13]).unwrap(), // Kind:30333 latest
        Event::from_json(EVENTS[12]).unwrap(), // Kind:5 deletion
        Event::from_json(EVENTS[8]).unwrap(),  // Kind:5 coordinate deletion
        Event::from_json(EVENTS[6]).unwrap(),  // Kind:32122 latest
        Event::from_json(EVENTS[5]).unwrap(),  // Kind:32122 from different author
        Event::from_json(EVENTS[4]).unwrap(),  // Kind:32122 from different author
        Event::from_json(EVENTS[1]).unwrap(),  // Kind:32121
        Event::from_json(EVENTS[0]).unwrap(),  // Kind:1 text note
    ];

    let actual = db.query(Filter::new()).await.unwrap().to_vec();
    assert_eq!(actual, expected_output);
    assert_eq!(db.count_all().await, 8); // 8 events after deletion validation
}
