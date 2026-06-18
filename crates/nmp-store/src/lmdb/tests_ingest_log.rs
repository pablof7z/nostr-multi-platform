//! Ingest-log smoke tests for the LMDB backend (ADR-0058 step-1).

#![cfg(feature = "lmdb-backend")]

use crate::events::EventStore;
use crate::ingest_log::ScanLogResult;
use crate::lmdb::LmdbEventStore;

fn make_store() -> (LmdbEventStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = LmdbEventStore::open(dir.path()).unwrap();
    (store, dir)
}

#[test]
fn empty_store_returns_zero_latest_seq() {
    let (store, _dir) = make_store();
    assert_eq!(store.latest_ingest_seq().unwrap(), 0);
}

#[test]
fn empty_store_returns_none_oldest_seq() {
    let (store, _dir) = make_store();
    assert_eq!(store.oldest_available_seq().unwrap(), None);
}

#[test]
fn empty_store_scan_returns_empty_page() {
    let (store, _dir) = make_store();
    let result = store.scan_log_since_seq(0, 100).unwrap();
    match result {
        ScanLogResult::Page(page) => {
            assert!(page.entries.is_empty());
            assert!(!page.has_more);
        }
        ScanLogResult::Gap(_) => panic!("expected Page, got Gap"),
    }
}
