// Copyright (c) 2024 Michael Dilger
// Copyright (c) 2022-2023 Yuki Kishimoto
// Copyright (c) 2023-2025 Rust Nostr Developers
// Distributed under the MIT software license

//! Tests for the LMDB event store.

use tempfile::TempDir;

use nostr::event::borrow::EventBorrow;
use nostr::{Event, EventBuilder, Filter, Keys, Kind, Timestamp};
use nostr_database::FlatBufferBuilder;

use super::{Lmdb, StoreAnomalySnapshot, DB_VERSION, DB_VERSION_KEY};

fn create_test_event(kind: u16, created_at: u64) -> Event {
    let keys = Keys::generate();
    EventBuilder::new(Kind::from(kind), "test content")
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(&keys)
        .unwrap()
}

#[test]
fn test_migration_v1_to_v2() {
    // Create a temporary directory for the test database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    // Step 1: Create a v1 database (without kc_index and version)
    {
        let lmdb = Lmdb::new(db_path, 1024 * 1024 * 100, 126, 0).unwrap();
        let mut txn = lmdb.write_txn().unwrap();
        let mut fbb = FlatBufferBuilder::new();

        // Insert some test events with different kinds
        let event1 = create_test_event(1, 1000);
        let event2 = create_test_event(1, 1001);
        let event3 = create_test_event(3, 1002);
        let event4 = create_test_event(5, 1003);

        lmdb.store(&mut txn, &mut fbb, &event1).unwrap();
        lmdb.store(&mut txn, &mut fbb, &event2).unwrap();
        lmdb.store(&mut txn, &mut fbb, &event3).unwrap();
        lmdb.store(&mut txn, &mut fbb, &event4).unwrap();

        // Manually clear kc_index and set version to 1 to simulate v1 database
        lmdb.kc_index.clear(&mut txn).unwrap();
        lmdb.metadata.put(&mut txn, DB_VERSION_KEY, &1u64).unwrap();

        txn.commit().unwrap();
    }

    // Step 2: Reopen the database - this should trigger migration
    {
        let lmdb = Lmdb::new(db_path, 1024 * 1024 * 100, 126, 0).unwrap();
        let txn = lmdb.read_txn().unwrap();

        // Verify version was updated
        let version = lmdb.metadata.get(&txn, DB_VERSION_KEY).unwrap();
        assert_eq!(version, Some(DB_VERSION));

        // Verify kc_index was populated by querying by kind
        let filter = Filter::new().kind(Kind::from(1));
        let results: Vec<EventBorrow> = lmdb.query(&txn, filter).unwrap().collect();
        assert_eq!(results.len(), 2, "Should find 2 events of kind 1");

        let filter = Filter::new().kind(Kind::from(3));
        let results: Vec<EventBorrow> = lmdb.query(&txn, filter).unwrap().collect();
        assert_eq!(results.len(), 1, "Should find 1 event of kind 3");

        let filter = Filter::new().kind(Kind::from(5));
        let results: Vec<EventBorrow> = lmdb.query(&txn, filter).unwrap().collect();
        assert_eq!(results.len(), 1, "Should find 1 event of kind 5");

        // Verify kc_index has entries
        let kc_count = lmdb.kc_index.len(&txn).unwrap();
        assert_eq!(kc_count, 4, "kc_index should have 4 entries");
    }
}

#[test]
fn test_migration_new_database() {
    // Create a new database from scratch
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    let lmdb = Lmdb::new(db_path, 1024 * 1024 * 100, 126, 0).unwrap();
    let txn = lmdb.read_txn().unwrap();

    // Verify version is set to current
    let version = lmdb.metadata.get(&txn, DB_VERSION_KEY).unwrap();
    assert_eq!(version, Some(DB_VERSION));
}

#[test]
fn test_migration_version_too_new() {
    use super::super::error::{Error, MigrationError};

    // Create a temporary directory for the test database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    // Create a database with a future version
    {
        let lmdb = Lmdb::new(db_path, 1024 * 1024 * 100, 126, 0).unwrap();
        let mut txn = lmdb.write_txn().unwrap();

        // Set version to something higher than current
        lmdb.metadata
            .put(&mut txn, DB_VERSION_KEY, &999u64)
            .unwrap();
        txn.commit().unwrap();
    }

    // Try to reopen - should fail
    let result = Lmdb::new(db_path, 1024 * 1024 * 100, 126, 0);
    assert!(matches!(
        result.unwrap_err(),
        Error::Migration(MigrationError::NewerVersion { .. })
    ));
}

/// V-69 regression: a ci_index entry that points to a missing event row
/// (orphan / dangling index pointer) must increment the
/// `anomaly_orphan_index_entries` counter and be silently skipped rather
/// than causing an undetectable silent result omission.
///
/// Corruption is simulated by storing an event (which populates ci_index),
/// then deleting only the event row from the `events` database while
/// leaving the ci_index entry intact.  An empty-filter query routes
/// through `query_by_scraping`, which is the path that contained the
/// original `.ok()??` double-swallow.
#[test]
fn test_v69_orphan_index_increments_anomaly_counter() {
    let temp_dir = TempDir::new().unwrap();
    let lmdb = Lmdb::new(temp_dir.path(), 1024 * 1024 * 100, 126, 0).unwrap();
    let mut fbb = FlatBufferBuilder::new();

    // Store one normal event and one that will become the orphan.
    let normal_event = create_test_event(1, 2000);
    let orphan_event = create_test_event(1, 1000);

    {
        let mut txn = lmdb.write_txn().unwrap();
        lmdb.store(&mut txn, &mut fbb, &normal_event).unwrap();
        lmdb.store(&mut txn, &mut fbb, &orphan_event).unwrap();
        txn.commit().unwrap();
    }

    // Verify baseline: no anomalies yet.
    let snap_before = lmdb.store_anomaly_snapshot();
    assert_eq!(
        snap_before,
        StoreAnomalySnapshot::default(),
        "no anomalies expected before corruption is introduced"
    );

    // Simulate corruption: delete ONLY the event row for `orphan_event`,
    // leaving its ci_index entry dangling.
    {
        let mut txn = lmdb.write_txn().unwrap();
        lmdb.events
            .delete(&mut txn, orphan_event.id.as_bytes())
            .unwrap();
        txn.commit().unwrap();
    }

    // An empty filter routes through query_by_scraping (no ids/authors/
    // kinds/tags → QueryFilterPattern::Scraping).
    {
        let txn = lmdb.read_txn().unwrap();
        let results: Vec<_> = lmdb.query(&txn, Filter::new()).unwrap().collect();

        // Only the normal event should be returned — the orphan is skipped.
        assert_eq!(
            results.len(),
            1,
            "only the non-orphaned event should be returned"
        );
        assert_eq!(
            results[0].id,
            normal_event.id.as_bytes(),
            "the surviving event should be the non-orphaned one"
        );

        txn.commit().unwrap();
    }

    // The orphan counter must have been incremented exactly once.
    let snap_after = lmdb.store_anomaly_snapshot();
    assert_eq!(
        snap_after.orphan_index_entries, 1,
        "one orphan ci_index entry should have been detected and counted"
    );
    assert_eq!(
        snap_after.unresolvable_events, 0,
        "no unresolvable-event anomalies expected"
    );
}
