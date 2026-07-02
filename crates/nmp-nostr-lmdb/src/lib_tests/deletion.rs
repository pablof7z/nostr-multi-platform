//! Deletion behavior: filter-based bulk delete (NIP-09 `kind` deletion
//! filters), id-targeted deletion events, and the regression fix ensuring
//! deleted events stay absent from both id- and author/kind-scoped queries.

use std::time::Duration;

use async_utility::time;

use crate::*;

use super::fixtures::{decode_events, setup_db, TempDatabase};

#[tokio::test]
async fn test_delete_by_filter() {
    let (db, _temp_dir) = setup_db().await;
    let events = decode_events();

    // Save all events
    for event in &events {
        db.save_event(event).await.expect("Failed to save event");
    }

    // Count before delete (8 visible after processing deletions/replacements)
    let count_before = db
        .count(Filter::new())
        .await
        .expect("Failed to count events");
    assert_eq!(count_before, 8);

    // Delete text notes
    let delete_filter = Filter::new().kind(Kind::TextNote);
    db.delete(delete_filter)
        .await
        .expect("Failed to delete events");

    // Count after delete (text notes: indices 0,4,13 - but 0 is deleted = 2 visible text notes deleted)
    let count_after = db
        .count(Filter::new())
        .await
        .expect("Failed to count events");
    assert_eq!(count_after, 7); // 8 - 1 text note = 7

    // Verify no text notes remain
    let text_notes = db
        .query(Filter::new().kind(Kind::TextNote))
        .await
        .expect("Failed to query");
    assert_eq!(text_notes.len(), 0);
}

#[tokio::test]
async fn test_event_deletion() {
    let (db, _temp_dir) = setup_db().await;
    let keys = Keys::generate();

    // Create events to delete
    let event1 = EventBuilder::text_note("To be deleted 1")
        .sign_with_keys(&keys)
        .expect("Failed to sign");
    let event2 = EventBuilder::text_note("To be deleted 2")
        .sign_with_keys(&keys)
        .expect("Failed to sign");

    db.save_event(&event1).await.expect("Failed to save event");
    db.save_event(&event2).await.expect("Failed to save event");

    // Create deletion event
    let deletion = EventBuilder::delete(EventDeletionRequest::new().id(event1.id).id(event2.id))
        .sign_with_keys(&keys)
        .expect("Failed to sign");

    db.save_event(&deletion)
        .await
        .expect("Failed to save deletion");

    // Sleep to ensure deletion is processed in the ingester
    time::sleep(Duration::from_millis(50)).await;

    // Check events are marked as deleted
    let status1 = db
        .check_id(&event1.id)
        .await
        .expect("Failed to check event");
    let status2 = db
        .check_id(&event2.id)
        .await
        .expect("Failed to check event");

    // Deleted events return Deleted status
    // (even though they're physically removed from the database)
    assert_eq!(status1, DatabaseEventStatus::Deleted);
    assert_eq!(status2, DatabaseEventStatus::Deleted);
}

#[tokio::test]
async fn test_kind5_deletion_query_bug_fix() {
    let db = TempDatabase::new();
    let keys = Keys::generate();

    // Create and save an event
    let event = EventBuilder::text_note("Test event")
        .sign_with_keys(&keys)
        .expect("Failed to sign");

    let status = db.save_event(&event).await.expect("Failed to save event");
    assert!(matches!(status, SaveEventStatus::Success));

    // Sleep to ensure it's committed
    time::sleep(Duration::from_millis(50)).await;

    // Verify it exists with ID filter
    let before_by_id = db
        .query(Filter::new().id(event.id))
        .await
        .expect("Failed to query");
    assert_eq!(before_by_id.len(), 1);

    // Verify it exists with author-kind filter
    let before_by_author = db
        .query(Filter::new().author(keys.public_key()).kind(Kind::TextNote))
        .await
        .expect("Failed to query");
    assert_eq!(before_by_author.len(), 1);

    // Create and save a Kind 5 deletion event
    let deletion_event = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(event.id))
        .sign_with_keys(&keys)
        .expect("Failed to sign");

    let del_status = db
        .save_event(&deletion_event)
        .await
        .expect("Failed to save deletion");
    assert!(matches!(del_status, SaveEventStatus::Success));

    // Sleep to ensure deletion is processed
    time::sleep(Duration::from_millis(100)).await;

    // Query for the deleted event by ID - should be empty after fix
    let after_by_id = db
        .query(Filter::new().id(event.id))
        .await
        .expect("Failed to query");
    assert_eq!(
        after_by_id.len(),
        0,
        "Deleted event should not be returned in ID query"
    );

    // Query for the deleted event by author-kind - should be empty after fix
    let after_by_author = db
        .query(Filter::new().author(keys.public_key()).kind(Kind::TextNote))
        .await
        .expect("Failed to query");
    assert_eq!(
        after_by_author.len(),
        0,
        "Deleted event should not be returned in author-kind query"
    );

    // The deletion event itself should still be queryable
    let deletion_events = db
        .query(Filter::new().kind(Kind::EventDeletion))
        .await
        .expect("Failed to query");
    assert_eq!(
        deletion_events.len(),
        1,
        "Deletion event should remain queryable"
    );
}
