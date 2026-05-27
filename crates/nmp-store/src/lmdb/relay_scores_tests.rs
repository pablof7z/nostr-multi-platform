//! TDD gate-tests for the `relay-author-scores-v1` LMDB sub-db (W2).
//!
//! Tests exercise `relay_scores::{load_all_raw, put_batch_raw, encode_key}`
//! directly (unit) and via open/reopen cycles (integration). All gated on
//! `lmdb-backend` so the file is dead without the feature.

#![cfg(all(test, feature = "lmdb-backend"))]

use tempfile::tempdir;

use crate::LmdbEventStore;

use super::relay_scores::{encode_key, load_all_raw, put_batch_raw};

/// Helper: open a fresh LmdbEventStore, returning (store, dir).
fn open_tmp() -> (LmdbEventStore, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let store = LmdbEventStore::open(dir.path()).expect("open");
    (store, dir)
}

/// §W2 T1 — write one cell, re-open the store, assert the cell is intact.
///
/// Asserts:
/// - `put_batch_raw` persists `(pubkey_bytes, canonical_url, successes, failures, last_used_unix_s)`
/// - `load_all_raw` returns the identical byte tuple after a fresh open
#[test]
fn roundtrip_persists_one_cell() {
    let dir = tempdir().expect("tempdir");
    let pubkey: [u8; 32] = [0xab; 32];
    let relay_url = "wss://relay.example.com";

    // Write
    {
        let store = LmdbEventStore::open(dir.path()).expect("open");
        let batch = vec![(pubkey, relay_url.to_string(), 7u32, 2u32, 1_700_000_000u64)];
        put_batch_raw(&store, batch).expect("put_batch_raw");
    }

    // Re-open and assert
    {
        let store = LmdbEventStore::open(dir.path()).expect("re-open");
        let rows = load_all_raw(&store).expect("load_all_raw");
        assert_eq!(rows.len(), 1, "expected exactly one cell");
        let (pk, url, s, f, ts) = &rows[0];
        assert_eq!(pk, &pubkey);
        assert_eq!(url, relay_url);
        assert_eq!(*s, 7);
        assert_eq!(*f, 2);
        assert_eq!(*ts, 1_700_000_000);
    }
}

/// §W2 T2 — schema-bump resets the table.
///
/// The sub-db is keyed by name `relay-author-scores-v1`. A hypothetical v2
/// (`relay-author-scores-v2`) would be a distinct LMDB named database — any
/// read against the v2 name returns empty even though v1 rows exist. This
/// test confirms the name-based isolation is exact.
///
/// (The production "bump" path never migrates rows; it just opens the new
/// name and the old rows are invisible — §5 E6.)
#[test]
fn schema_bump_resets_table() {
    use super::relay_scores::load_all_raw_with_name;

    let dir = tempdir().expect("tempdir");
    let pubkey: [u8; 32] = [0xcd; 32];
    let relay_url = "wss://relay.example.com";

    // Write a v1 cell.
    {
        let store = LmdbEventStore::open(dir.path()).expect("open");
        let batch = vec![(pubkey, relay_url.to_string(), 1u32, 0u32, 1_000u64)];
        put_batch_raw(&store, batch).expect("put_batch_raw");
    }

    // Re-open and confirm v1 has the row.
    {
        let store = LmdbEventStore::open(dir.path()).expect("re-open");
        let rows = load_all_raw(&store).expect("v1 load");
        assert_eq!(rows.len(), 1);
    }

    // A "v2" read using a different sub-db name sees zero rows.
    {
        let store = LmdbEventStore::open(dir.path()).expect("re-open for v2");
        let rows = load_all_raw_with_name(&store, "relay-author-scores-v2").expect("v2 load");
        assert_eq!(
            rows.len(),
            0,
            "v2 name must be empty (schema-bump isolation)"
        );
    }
}

/// §W2 T5 — URL of length > 255 is silently skipped; no panic.
///
/// `encode_key` rejects URLs whose UTF-8 byte length exceeds 255 (the `u8`
/// length prefix limit per §8.9). The key encodes as `None`.
///
/// This test exercises `encode_key` only (pure, no LMDB env needed).
/// The LMDB round-trip path (T1/T2) tests persistence end-to-end.
#[test]
fn url_over_255_chars_rejected_without_panic() {
    let pubkey: [u8; 32] = [0x11; 32];

    // 256-byte URL
    let padding = "x".repeat(256 - "wss://relay.example.com/".len() + 1);
    let long_url = format!("wss://relay.example.com/{}", padding);
    assert!(
        long_url.len() > 255,
        "test setup: URL must exceed 255 bytes"
    );

    // encode_key must return None for a too-long URL (pure computation — no disk I/O)
    assert!(
        encode_key(&pubkey, &long_url).is_none(),
        "encode_key must return None for URL > 255 bytes"
    );

    // A 255-byte URL must be accepted.
    let exactly_255 = "wss://".to_string() + &"a".repeat(255 - "wss://".len());
    assert_eq!(exactly_255.len(), 255);
    assert!(
        encode_key(&pubkey, &exactly_255).is_some(),
        "encode_key must accept a 255-byte URL"
    );
}
