//! Auxiliary query and maintenance operations: direct id lookup, full
//! database wipe, negentropy sync item enumeration, and NIP-50 full-text
//! search.

use std::collections::HashSet;

use crate::*;

use super::fixtures::{decode_events, setup_db, TempDatabase};

#[tokio::test]
async fn test_event_by_id() {
    let db = TempDatabase::new();

    let added_events: usize = db.add_random_events().await;

    let (_keys, expected_event) = db.add_event(EventBuilder::text_note("Test")).await;

    let event = db.event_by_id(&expected_event.id).await.unwrap().unwrap();
    assert_eq!(event, expected_event);

    // Check if number of events in database match the expected
    assert_eq!(db.count_all().await, added_events + 1)
}

#[tokio::test]
async fn test_wipe_database() {
    let (db, _temp_dir) = setup_db().await;
    let events = decode_events();

    // Save all events
    for event in &events {
        db.save_event(event).await.expect("Failed to save event");
    }

    // Verify events exist (7 visible after processing)
    let count = db
        .count(Filter::new())
        .await
        .expect("Failed to count events");
    assert_eq!(count, 8);

    // Wipe database
    db.wipe().await.expect("Failed to wipe database");

    // Verify database is empty
    let count_after = db
        .count(Filter::new())
        .await
        .expect("Failed to count events");
    assert_eq!(count_after, 0);
}

#[tokio::test]
async fn test_negentropy_items() {
    let (db, _temp_dir) = setup_db().await;
    let events = decode_events();

    // Save all events
    for event in &events {
        db.save_event(event).await.expect("Failed to save event");
    }

    // Get negentropy items (7 visible events)
    let items = db
        .negentropy_items(Filter::new())
        .await
        .expect("Failed to get negentropy items");

    assert_eq!(items.len(), 8);

    // Verify items are from the original events
    let event_ids: HashSet<EventId> = events.iter().map(|e| e.id).collect();

    for (id, _timestamp) in items {
        assert!(
            event_ids.contains(&id),
            "Unexpected event ID in negentropy items"
        );
    }
}

#[tokio::test]
async fn test_full_text_search() {
    let db = TempDatabase::new();

    let _added_events: usize = db.add_random_events().await;

    let events = db.query(Filter::new().search("Account A")).await.unwrap();
    assert_eq!(events.len(), 1);

    let events = db.query(Filter::new().search("account a")).await.unwrap();
    assert_eq!(events.len(), 1);

    let events = db.query(Filter::new().search("text note")).await.unwrap();
    assert_eq!(events.len(), 2);

    let events = db.query(Filter::new().search("notes")).await.unwrap();
    assert_eq!(events.len(), 0);

    let events = db.query(Filter::new().search("hola")).await.unwrap();
    assert_eq!(events.len(), 0);
}
