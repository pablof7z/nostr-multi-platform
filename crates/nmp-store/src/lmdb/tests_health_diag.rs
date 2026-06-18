//! Health-diagnostic tests for typed LMDB `StoreError` variants (#1521).
//!
//! Tests:
//!   1. Classifier unit tests — heed MdbError → StoreError mapping and
//!      Display content safety.
//!   2. Map-full integration test — insert events until `StoreError::MapFull`
//!      is returned by the production insert path.
//!   3. Reader-exhaustion integration test — open two concurrent read
//!      transactions against a store configured with max_readers=1.
//!
//! All tests are gated on `#[cfg(all(test, feature = "lmdb-backend"))]`
//! (see mod.rs registration) so they are invisible when the feature is off.

#![cfg(feature = "lmdb-backend")]

use heed::MdbError;

use super::open::open_impl_with_limits;
use super::open_error::classify_heed_err;
use super::test_fixtures::{signed_event, verified};
use crate::types::StoreError;
use crate::EventStore;

// ─── classifier unit tests ────────────────────────────────────────────────────

#[test]
fn classifier_readers_full() {
    let e = heed::Error::Mdb(MdbError::ReadersFull);
    let r = classify_heed_err(e, 1024, 7);
    assert!(
        matches!(r, StoreError::ReaderExhaustion { max_readers: 7 }),
        "expected ReaderExhaustion{{max_readers:7}}, got {r:?}"
    );
}

#[test]
fn classifier_map_full() {
    let e = heed::Error::Mdb(MdbError::MapFull);
    let r = classify_heed_err(e, 65536, 10);
    assert!(
        matches!(r, StoreError::MapFull { map_size_bytes: 65536 }),
        "expected MapFull{{map_size_bytes:65536}}, got {r:?}"
    );
}

#[test]
fn classifier_corrupted() {
    let cases = [MdbError::Corrupted, MdbError::Panic, MdbError::Invalid, MdbError::PageNotFound];
    for mdb in cases {
        let e = heed::Error::Mdb(mdb);
        let r = classify_heed_err(e, 1024, 4);
        assert!(
            matches!(r, StoreError::CorruptEnv(_)),
            "expected CorruptEnv for {mdb:?}, got {r:?}"
        );
    }
}

#[test]
fn classifier_version_mismatch() {
    let e = heed::Error::Mdb(MdbError::VersionMismatch);
    let r = classify_heed_err(e, 1024, 4);
    assert!(
        matches!(r, StoreError::VersionMismatch { .. }),
        "expected VersionMismatch, got {r:?}"
    );
}

#[test]
fn classifier_display_no_sensitive_data() {
    // Display strings must not contain file-system paths or sensitive tokens.
    let cases: &[StoreError] = &[
        StoreError::ReaderExhaustion { max_readers: 126 },
        StoreError::MapFull { map_size_bytes: 1024 },
        StoreError::CorruptEnv("lmdb environment corrupted or invalid".into()),
        StoreError::VersionMismatch { detail: "lmdb binary version mismatch".into() },
    ];
    for err in cases {
        let msg = err.to_string();
        assert!(!msg.contains('/'), "Display leaks path separator in: {msg}");
        assert!(!msg.contains("secret"), "Display leaks 'secret' in: {msg}");
    }
}

// ─── map-full integration test ────────────────────────────────────────────────

/// Open an LMDB store with a small map and insert events until
/// `StoreError::MapFull` is returned.  Asserts the typed variant rather than
/// an `Io("...")` string.
///
/// 1 MiB is large enough for LMDB to initialise all sub-dbs but small enough
/// that ~100–300 signed events will exhaust it reliably.
#[test]
fn insert_until_map_full() {
    let dir = tempfile::tempdir().expect("tempdir");
    // 1 MiB map — large enough to open all sub-dbs, small enough to fill up.
    let store = open_impl_with_limits(dir.path(), 1024 * 1024, 126)
        .expect("open with tiny map");

    let relay: String = "wss://test.relay/".into();
    let mut map_full_seen = false;

    for i in 0u64..300 {
        let raw = signed_event(1, 1_000_000 + i, "x", None);
        match store.insert(verified(raw), &relay, 1_000_000_000) {
            Ok(_) => {}
            Err(StoreError::MapFull { map_size_bytes }) => {
                assert_eq!(
                    map_size_bytes,
                    1024 * 1024,
                    "MapFull carries wrong map_size_bytes"
                );
                map_full_seen = true;
                break;
            }
            Err(e) => panic!("unexpected error after {i} inserts: {e:?}"),
        }
    }

    assert!(map_full_seen, "expected StoreError::MapFull but never saw it after 300 inserts");
}

// ─── reader-exhaustion integration test ──────────────────────────────────────

/// Open a store with max_readers=1 and attempt to open two concurrent read
/// transactions.  The second open must produce `StoreError::ReaderExhaustion`.
///
/// LMDB reader slots are per-process, not per-env clone, so we use the raw
/// `heed::Env` from the store's inner handle to open the second txn.
#[test]
fn reader_exhaustion() {
    let dir = tempfile::tempdir().expect("tempdir");
    // max_readers=1: only one concurrent reader slot.
    let store = open_impl_with_limits(dir.path(), super::open::MAP_SIZE, 1)
        .expect("open with max_readers=1");

    let inner = store.inner_for_test();
    let env = &inner.env;

    // First read txn — must succeed.
    let _txn1 = env.read_txn().expect("first read txn");

    // Second read txn — must fail with ReadersFull.
    let err = match env.read_txn() {
        Ok(_) => panic!("expected reader exhaustion on second txn but it succeeded"),
        Err(e) => e,
    };
    let classified = classify_heed_err(err, super::open::MAP_SIZE, 1);
    assert!(
        matches!(classified, StoreError::ReaderExhaustion { max_readers: 1 }),
        "expected ReaderExhaustion{{max_readers:1}}, got {classified:?}"
    );
}
