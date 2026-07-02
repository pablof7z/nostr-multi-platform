//! Replaceable (kind 0/metadata) and parameterized-addressable event
//! replacement: newer-timestamp events supersede older ones, replaced
//! events disappear from id lookup, and stale-timestamp writes are
//! rejected outright.

use std::time::Duration;

use crate::*;

use super::fixtures::TempDatabase;

#[tokio::test]
async fn test_replaceable_events() {
    let (db, _temp_dir) = super::fixtures::setup_db().await;
    let keys = Keys::generate();

    // Create first replaceable event (kind 0 - metadata)
    let metadata1 = Metadata::new().name("First");
    let event1 = EventBuilder::metadata(&metadata1)
        .custom_created_at(Timestamp::from_secs(1000))
        .sign_with_keys(&keys)
        .expect("Failed to sign");

    db.save_event(&event1).await.expect("Failed to save event");

    // Create newer replaceable event with later timestamp
    let metadata2 = Metadata::new().name("Second");
    let event2 = EventBuilder::metadata(&metadata2)
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .expect("Failed to sign");

    db.save_event(&event2).await.expect("Failed to save event");

    // Query metadata events
    let filter = Filter::new().author(keys.public_key()).kind(Kind::Metadata);
    let results = db.query(filter).await.expect("Failed to query");

    // Should only have the newer event
    assert_eq!(results.len(), 1);
    // Verify it's the newer event by content
    let result_event = results.first().unwrap();
    assert!(result_event.content.contains("Second"));
}

#[tokio::test]
async fn test_addressable_events() {
    let (db, _temp_dir) = super::fixtures::setup_db().await;
    let keys = Keys::generate();

    // Create first addressable event
    let event1 = EventBuilder::new(Kind::from(32121), "Content 1")
        .tag(Tag::identifier("test-id"))
        .custom_created_at(Timestamp::from_secs(1000))
        .sign_with_keys(&keys)
        .expect("Failed to sign");

    db.save_event(&event1).await.expect("Failed to save event");

    // Create newer addressable event with same identifier
    let event2 = EventBuilder::new(Kind::from(32121), "Content 2")
        .tag(Tag::identifier("test-id"))
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .expect("Failed to sign");

    db.save_event(&event2).await.expect("Failed to save event");

    // Query addressable events
    let filter = Filter::new()
        .author(keys.public_key())
        .kind(Kind::from(32121));
    let results = db.query(filter).await.expect("Failed to query");

    // Should only have the newer event
    assert_eq!(results.len(), 1);
    // Verify it's the newer event by content
    let result_event = results.first().unwrap();
    assert_eq!(result_event.content, "Content 2");
}

#[tokio::test]
async fn test_replaceable_event() {
    let db = TempDatabase::new();

    let added_events: usize = db.add_random_events().await;

    let now = Timestamp::now();
    let metadata = Metadata::new()
        .name("my-account")
        .display_name("My Account");

    let (keys, expected_event) = db
        .add_event(
            EventBuilder::metadata(&metadata).custom_created_at(now - Duration::from_secs(120)),
        )
        .await;

    // Test event by ID
    let event = db.event_by_id(&expected_event.id).await.unwrap().unwrap();
    assert_eq!(event, expected_event);

    // Test filter query
    let events = db
        .query(Filter::new().author(keys.public_key).kind(Kind::Metadata))
        .await
        .unwrap();
    assert_eq!(events.to_vec(), vec![expected_event.clone()]);

    // Check if number of events in database match the expected
    assert_eq!(db.count_all().await, added_events + 1);

    // Replace previous event
    let (new_expected_event, status) = db
        .add_event_with_keys(
            EventBuilder::metadata(&metadata).custom_created_at(now),
            &keys,
        )
        .await;
    assert!(status.is_success());

    // Test event by ID (MUST be None because replaced)
    assert!(db.event_by_id(&expected_event.id).await.unwrap().is_none());

    // Test event by ID
    let event = db
        .event_by_id(&new_expected_event.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event, new_expected_event);

    // Test filter query
    let events = db
        .query(Filter::new().author(keys.public_key).kind(Kind::Metadata))
        .await
        .unwrap();
    assert_eq!(events.to_vec(), vec![new_expected_event]);

    // Check if number of events in database match the expected
    assert_eq!(db.count_all().await, added_events + 1);
}

#[tokio::test]
async fn test_param_replaceable_event() {
    let db = TempDatabase::new();

    let added_events: usize = db.add_random_events().await;

    let now = Timestamp::now();

    let (keys, expected_event) = db
        .add_event(
            EventBuilder::new(Kind::Custom(33_333), "")
                .tag(Tag::identifier("my-id-a"))
                .custom_created_at(now - Duration::from_secs(120)),
        )
        .await;
    let coordinate = Coordinate::new(Kind::from(33_333), keys.public_key).identifier("my-id-a");

    // Test event by ID
    let event = db.event_by_id(&expected_event.id).await.unwrap().unwrap();
    assert_eq!(event, expected_event);

    // Test filter query
    let events = db.query(coordinate.clone().into()).await.unwrap();
    assert_eq!(events.to_vec(), vec![expected_event.clone()]);

    // Check if number of events in database match the expected
    assert_eq!(db.count_all().await, added_events + 1);

    // Replace previous event
    let (new_expected_event, status) = db
        .add_event_with_keys(
            EventBuilder::new(Kind::Custom(33_333), "Test replace")
                .tag(Tag::identifier("my-id-a"))
                .custom_created_at(now),
            &keys,
        )
        .await;
    assert!(status.is_success());

    // Test event by ID (MUST be None` because replaced)
    assert!(db.event_by_id(&expected_event.id).await.unwrap().is_none());

    // Test event by ID
    let event = db
        .event_by_id(&new_expected_event.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event, new_expected_event);

    // Test filter query
    let events = db.query(coordinate.into()).await.unwrap();
    assert_eq!(events.to_vec(), vec![new_expected_event]);

    // Check if number of events in database match the expected
    assert_eq!(db.count_all().await, added_events + 1);

    // Trey to add param replaceable event with older timestamp (MUSTN'T be stored)
    let (_, status) = db
        .add_event_with_keys(
            EventBuilder::new(Kind::Custom(33_333), "Test replace 2")
                .tag(Tag::identifier("my-id-a"))
                .custom_created_at(now - Duration::from_secs(2000)),
            &keys,
        )
        .await;
    assert!(!status.is_success());
}
