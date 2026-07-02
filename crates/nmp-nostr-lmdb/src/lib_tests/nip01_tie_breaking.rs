//! NIP-01 tie-breaking for replaceable/addressable events that share an
//! identical `created_at`: the event with the lexicographically smallest id
//! must win, regardless of insertion order.

use crate::*;

use super::fixtures::TempDatabase;

#[tokio::test]
async fn test_nip01_replaceable_events_with_identical_timestamps() {
    let db = TempDatabase::new();

    // Parse the two events with identical timestamps but different IDs
    let event1_json = r#"{"kind":0,"id":"b39eda8475d345d4da75418f0ee4a9ec183eb0483634cfdc8415cefdf5c02b96","pubkey":"79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798","created_at":1754066538,"tags":[],"content":"smallest id","sig":"22e9f94de060c8e0a958b8fbc42914fab12c90be5ea9153aa11f92ed8c38c18a3374221fe37cb40b461d391e8ee92d6dd5083b0be8e146bf90af694560f18e17"}"#;
    let event2_json = r#"{"kind":0,"id":"eedcb07adabb380e853815534568e05cc5678bc8f9d8cf3dbee8513d37f1c47f","pubkey":"79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798","created_at":1754066538,"tags":[],"content":"biggest id","sig":"b127afb6b7967b483bffe0d8ca21335509820bbfc37ba09874cc931c6c8c97dec4d44ac5d1071973d3b337c6fe614a8ca2cdc1e89c3c68cd557f4a2eca90cab3"}"#;

    let event1 = Event::from_json(event1_json).expect("Failed to parse event1");
    let event2 = Event::from_json(event2_json).expect("Failed to parse event2");

    // Confirm both events have the same timestamp
    assert_eq!(event1.created_at, event2.created_at);
    assert_eq!(event1.created_at.as_secs(), 1754066538);

    // Confirm event1 has the smaller ID (lexicographically first)
    assert!(event1.id.to_string() < event2.id.to_string());

    // Test 1: Insert event1 first, then event2
    {
        let status1 = db.save_event(&event1).await.expect("Failed to save event1");
        assert!(matches!(status1, SaveEventStatus::Success));

        let status2 = db.save_event(&event2).await.expect("Failed to save event2");
        assert!(matches!(
            status2,
            SaveEventStatus::Rejected(RejectedReason::Replaced)
        ));

        // Query to see which event is stored
        let filter = Filter::new()
            .author(
                PublicKey::from_hex(
                    "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                )
                .unwrap(),
            )
            .kind(Kind::Metadata);

        let results = db.query(filter).await.expect("Failed to query");
        assert_eq!(results.len(), 1, "Should have exactly one metadata event");

        // According to NIP-01, event1 should be retained (smaller ID)
        assert_eq!(
            results.first().unwrap().id,
            event1.id,
            "NIP-01: Event with lowest ID should be retained"
        );
    }

    // Clean database
    db.wipe().await.expect("Failed to wipe database");

    // Test 2: Insert event2 first, then event1
    {
        let status2 = db.save_event(&event2).await.expect("Failed to save event2");
        assert!(matches!(status2, SaveEventStatus::Success));

        let status1 = db.save_event(&event1).await.expect("Failed to save event1");
        assert!(matches!(status1, SaveEventStatus::Success));

        // Query to see which event is stored
        let filter = Filter::new()
            .author(
                PublicKey::from_hex(
                    "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                )
                .unwrap(),
            )
            .kind(Kind::Metadata);

        let results = db.query(filter).await.expect("Failed to query");
        assert_eq!(results.len(), 1, "Should have exactly one metadata event");

        // According to NIP-01, event1 should be retained (smaller ID)
        assert_eq!(
            results.first().unwrap().id,
            event1.id,
            "NIP-01: Event with lowest ID should be retained"
        );
    }
}

#[tokio::test]
async fn test_nip01_addressable_events_with_identical_timestamps() {
    let db = TempDatabase::new();

    // Create two addressable events (kind 30023) with identical timestamps but different IDs
    let event1_json = r#"{"kind":30023,"id":"a11eda8475d345d4da75418f0ee4a9ec183eb0483634cfdc8415cefdf5c02b96","pubkey":"79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798","created_at":1754066538,"tags":[["d","article-123"],["title","Test Article"]],"content":"Article with smallest id","sig":"22e9f94de060c8e0a958b8fbc42914fab12c90be5ea9153aa11f92ed8c38c18a3374221fe37cb40b461d391e8ee92d6dd5083b0be8e146bf90af694560f18e17"}"#;
    let event2_json = r#"{"kind":30023,"id":"feedb07adabb380e853815534568e05cc5678bc8f9d8cf3dbee8513d37f1c47f","pubkey":"79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798","created_at":1754066538,"tags":[["d","article-123"],["title","Test Article"]],"content":"Article with biggest id","sig":"b127afb6b7967b483bffe0d8ca21335509820bbfc37ba09874cc931c6c8c97dec4d44ac5d1071973d3b337c6fe614a8ca2cdc1e89c3c68cd557f4a2eca90cab3"}"#;

    let event1 = Event::from_json(event1_json).expect("Failed to parse event1");
    let event2 = Event::from_json(event2_json).expect("Failed to parse event2");

    // Confirm both events have the same timestamp and d-tag
    assert_eq!(event1.created_at, event2.created_at);
    assert_eq!(event1.created_at.as_secs(), 1754066538);
    assert_eq!(event1.tags.identifier(), event2.tags.identifier());
    assert_eq!(event1.tags.identifier(), Some("article-123"));

    // Confirm event1 has the smaller ID (lexicographically first)
    assert!(event1.id.to_string() < event2.id.to_string());

    // Test 1: Insert event1 first, then event2
    {
        let status1 = db.save_event(&event1).await.expect("Failed to save event1");
        assert!(matches!(status1, SaveEventStatus::Success));

        let status2 = db.save_event(&event2).await.expect("Failed to save event2");
        assert!(matches!(
            status2,
            SaveEventStatus::Rejected(RejectedReason::Replaced)
        ));

        // Query to see which event is stored
        let filter = Filter::new()
            .author(
                PublicKey::from_hex(
                    "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                )
                .unwrap(),
            )
            .kind(Kind::Custom(30023));

        let results = db.query(filter).await.expect("Failed to query");
        assert_eq!(
            results.len(),
            1,
            "Should have exactly one addressable event"
        );

        // According to NIP-01, event1 should be retained (smaller ID)
        assert_eq!(
            results.first().unwrap().id,
            event1.id,
            "NIP-01: Event with lowest ID should be retained"
        );
    }

    // Clean database
    db.wipe().await.expect("Failed to wipe database");

    // Test 2: Insert event2 first, then event1
    {
        let status2 = db.save_event(&event2).await.expect("Failed to save event2");
        assert!(matches!(status2, SaveEventStatus::Success));

        let status1 = db.save_event(&event1).await.expect("Failed to save event1");
        assert!(matches!(status1, SaveEventStatus::Success));

        // Query to see which event is stored
        let filter = Filter::new()
            .author(
                PublicKey::from_hex(
                    "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                )
                .unwrap(),
            )
            .kind(Kind::Custom(30023));

        let results = db.query(filter).await.expect("Failed to query");
        assert_eq!(
            results.len(),
            1,
            "Should have exactly one addressable event"
        );

        // According to NIP-01, event1 should be retained (smaller ID)
        assert_eq!(
            results.first().unwrap().id,
            event1.id,
            "NIP-01: Event with lowest ID should be retained"
        );
    }
}
