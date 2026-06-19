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

/// Open an LMDB store with a small, fixed map and insert events until
/// `StoreError::MapFull` is returned.  Asserts the typed variant rather than
/// an `Io("...")` string.
///
/// **Page-size-agnostic by construction.** The map is opened at a fixed size
/// via `open_impl_with_limits` (no auto-grow), so writing more total bytes than
/// the map can hold MUST raise `MDB_MAP_FULL`. To guarantee that on ANY OS page
/// size (4 KiB on Linux CI, 16 KiB on macOS), each event carries a multi-KiB
/// content payload and the loop cap is chosen so the cumulative payload provably
/// exceeds the map size:
///
/// ```text
///   LOOP_CAP * PER_EVENT_PAYLOAD_BYTES  >  MAP_SIZE_BYTES
///   512      * 4096                     >  1_048_576
///   2_097_152 (2 MiB)                   >  1_048_576 (1 MiB)
/// ```
///
/// The earlier version inserted 300 tiny ("x") events into a 1 MiB map; on
/// macOS's 16 KiB pages, page-rounding overhead happened to exhaust the map, but
/// on Linux's 4 KiB pages those same tiny events fit inside 1 MiB and map-full
/// never triggered — so CI failed at the final assert. Sizing the trigger by
/// total payload bytes (not page-rounding luck) removes that page-size
/// dependency entirely.
#[test]
fn insert_until_map_full() {
    // Fixed map size with NO auto-grow: exceeding it must raise MDB_MAP_FULL.
    const MAP_SIZE_BYTES: usize = 1024 * 1024; // 1 MiB
    // Each event's `content` is this many bytes; the stored event (full JSON +
    // index entries) is strictly larger, so this is a conservative lower bound
    // on the bytes every insert commits to the map.
    const PER_EVENT_PAYLOAD_BYTES: usize = 4096; // 4 KiB
    // Generous cap that still guarantees the trigger; the loop breaks early once
    // MapFull is seen, so this only bounds the worst case.
    const LOOP_CAP: u64 = 512;
    // Compile-time proof that cumulative payload exceeds the map size, so
    // map-full is guaranteed on any page size: 512 * 4096 = 2 MiB > 1 MiB.
    const _: () = assert!(
        (LOOP_CAP as usize) * PER_EVENT_PAYLOAD_BYTES > MAP_SIZE_BYTES,
        "loop cap * per-event payload must exceed the map size to guarantee MapFull on any page size",
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_impl_with_limits(dir.path(), MAP_SIZE_BYTES, 126)
        .expect("open with tiny map");

    let content = "x".repeat(PER_EVENT_PAYLOAD_BYTES);
    let relay: String = "wss://test.relay/".into();
    let mut map_full_seen = false;

    for i in 0u64..LOOP_CAP {
        let raw = signed_event(1, 1_000_000 + i, &content, None);
        match store.insert(verified(raw), &relay, 1_000_000_000) {
            Ok(_) => {}
            Err(StoreError::MapFull { map_size_bytes }) => {
                assert_eq!(
                    map_size_bytes,
                    MAP_SIZE_BYTES as u64,
                    "MapFull carries wrong map_size_bytes"
                );
                map_full_seen = true;
                break;
            }
            Err(e) => panic!("unexpected error after {i} inserts: {e:?}"),
        }
    }

    assert!(
        map_full_seen,
        "expected StoreError::MapFull but never saw it within {LOOP_CAP} inserts of {PER_EVENT_PAYLOAD_BYTES}-byte payloads",
    );
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
