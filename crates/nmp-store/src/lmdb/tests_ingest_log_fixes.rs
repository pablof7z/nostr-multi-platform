//! Fix-verification tests for the LMDB ingest log (500-LOC cap split from
//! `tests_ingest_log.rs`).
//!
//! Covers:
//!   - BLOCKING 1: Duplicate kind:5 → no new seq, no new log row
//!   - BLOCKING 3: a-tag regular-replaceable target → removed + Deleted{Nip09}
//!   - BLOCKING 4: Append-time trim: after DEFAULT_LOG_MAX_ENTRIES+N appends,
//!                 gc_floor advances and scan below floor returns PullGap
//!   - SHOULD-FIX 6: Persisted format: version field + stable variant names

#![cfg(feature = "lmdb-backend")]

use crate::events::EventStore;
use crate::ingest_log::{DeleteReason, LogOp, ScanLogResult};
use crate::LmdbEventStore;

use super::test_fixtures::{open_tmp, signed_event, signed_event_with_keys, verified};

const TEST_RELAY: &str = "wss://test/";

// ── BLOCKING 1: duplicate kind:5 ─────────────────────────────────────────────

/// Re-delivering a kind:5 MUST NOT add a new log row. Already handled by
/// lmdb/insert_kind5.rs (`has_event` gate); this test keeps both backends in
/// lock-step.
#[test]
fn lmdb_kind5_duplicate_emits_no_new_seq_or_log_row() {
    let (store, _dir) = open_tmp();
    let keys = nostr::Keys::generate();

    let target = signed_event_with_keys(&keys, 1, 500, "doomed", None);
    store
        .insert(verified(target.clone()), &TEST_RELAY.into(), 500_000)
        .unwrap();

    use nostr::prelude::*;
    let target_id_bytes = target.id_bytes().expect("valid hex");
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(
            nostr::EventId::from_slice(&target_id_bytes).unwrap(),
        ))
        .custom_created_at(Timestamp::from_secs(600))
        .sign_with_keys(&keys)
        .expect("sign");
    let k5_json = k5.try_as_json().expect("json");
    let k5_raw: crate::types::RawEvent = serde_json::from_str(&k5_json).expect("parse");

    store
        .insert(verified(k5_raw.clone()), &TEST_RELAY.into(), 600_000)
        .unwrap();
    let seq_after_first = store.latest_ingest_seq().unwrap();
    let log_count_after_first = match store.scan_log_since_seq(0, 10_000).unwrap() {
        ScanLogResult::Page(p) => p.entries.len(),
        ScanLogResult::Gap(_) => panic!("unexpected gap"),
    };

    // Re-deliver the identical kind:5.
    store
        .insert(verified(k5_raw), &TEST_RELAY.into(), 601_000)
        .unwrap();

    assert_eq!(
        store.latest_ingest_seq().unwrap(),
        seq_after_first,
        "BLOCKING 1 (LMDB): duplicate kind:5 must NOT allocate a new seq"
    );
    match store.scan_log_since_seq(0, 10_000).unwrap() {
        ScanLogResult::Page(page) => {
            assert_eq!(
                page.entries.len(),
                log_count_after_first,
                "BLOCKING 1 (LMDB): duplicate kind:5 must NOT add a new log row"
            );
        }
        ScanLogResult::Gap(_) => panic!("unexpected gap"),
    }
}

// ── BLOCKING 3: a-tag regular-replaceable ────────────────────────────────────

/// kind:5 with a-tag targeting kind:0 (regular replaceable, empty d-tag)
/// MUST remove the target and emit Deleted{Nip09}.
/// Covered by lmdb/insert_kind5.rs:230 (is_replaceable branch).
#[test]
fn lmdb_kind5_atag_regular_replaceable_removes_target_and_logs() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    let profile = signed_event_with_keys(&keys, 0, 100, "my profile", None);
    let profile_id = profile.id_bytes().expect("valid hex");
    store
        .insert(verified(profile), &TEST_RELAY.into(), 100_000)
        .unwrap();

    let pubkey_hex = keys.public_key().to_hex();
    let coord_str = format!("0:{pubkey_hex}:");
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(vec!["a".to_string(), coord_str]).expect("tag"))
        .custom_created_at(Timestamp::from_secs(200))
        .sign_with_keys(&keys)
        .expect("sign");
    let k5_json = k5.try_as_json().expect("json");
    let k5_raw: crate::types::RawEvent = serde_json::from_str(&k5_json).expect("parse");
    store
        .insert(verified(k5_raw), &TEST_RELAY.into(), 200_000)
        .unwrap();

    assert!(
        store.get_by_id(&profile_id).unwrap().is_none(),
        "BLOCKING 3 (LMDB): kind:0 regular-replaceable must be removed by kind:5 a-tag"
    );

    match store.scan_log_since_seq(0, 100).unwrap() {
        ScanLogResult::Page(page) => {
            let has_deleted = page.entries.iter().any(|e| {
                matches!(
                    &e.op,
                    LogOp::Deleted { reason: DeleteReason::Nip09, target_id: tid }
                    if *tid == profile_id
                )
            });
            assert!(
                has_deleted,
                "BLOCKING 3 (LMDB): must emit Deleted{{Nip09}} for regular-replaceable target"
            );
        }
        ScanLogResult::Gap(_) => panic!("unexpected gap"),
    }
}

// ── BLOCKING 4: append-time trim ─────────────────────────────────────────────

/// After DEFAULT_LOG_MAX_ENTRIES + N appends, `oldest_available_seq` must have
/// advanced (gc_floor > 0) and scan below floor returns Gap.
///
/// Seeds last_seq to DEFAULT_LOG_MAX_ENTRIES - 1 via inner_for_test() so only
/// 2 inserts are needed to trigger the trim.
#[test]
fn lmdb_append_time_trim_advances_gc_floor() {
    use crate::ingest_log::DEFAULT_LOG_MAX_ENTRIES;

    let (store, _dir) = open_tmp();

    {
        let inner = store.inner_for_test();
        let mut txn = inner.env.write_txn().expect("write_txn");
        inner
            .ingest_meta
            .put(
                &mut txn,
                b"last_seq",
                &(DEFAULT_LOG_MAX_ENTRIES - 1).to_be_bytes(),
            )
            .expect("put last_seq");
        txn.commit().expect("commit");
    }

    // First insert: seq = DEFAULT_LOG_MAX_ENTRIES (retained == DEFAULT, no trim yet).
    let ev1 = signed_event(1, 1000, "first", None);
    store
        .insert(verified(ev1), &TEST_RELAY.into(), 1_000_000)
        .unwrap();
    assert_eq!(store.latest_ingest_seq().unwrap(), DEFAULT_LOG_MAX_ENTRIES);
    assert_eq!(
        store.oldest_available_seq().unwrap(),
        Some(DEFAULT_LOG_MAX_ENTRIES),
        "oldest_seq must equal DEFAULT_LOG_MAX_ENTRIES before trim"
    );

    // Second insert: seq = DEFAULT_LOG_MAX_ENTRIES + 1 → triggers trim.
    let ev2 = signed_event(1, 2000, "second", None);
    store
        .insert(verified(ev2), &TEST_RELAY.into(), 2_000_000)
        .unwrap();
    assert_eq!(
        store.latest_ingest_seq().unwrap(),
        DEFAULT_LOG_MAX_ENTRIES + 1
    );

    assert!(
        store.oldest_available_seq().unwrap().is_some(),
        "BLOCKING 4: oldest_available_seq must be Some after trim"
    );

    // Scan from 0 must be a Gap (0 < gc_floor).
    match store.scan_log_since_seq(0, 100).unwrap() {
        ScanLogResult::Gap(g) => {
            assert_eq!(g.requested_after_seq, 0);
            assert_eq!(
                g.first_available_seq, 2,
                "first_available = gc_floor + 1 = 2"
            );
        }
        ScanLogResult::Page(_) => panic!("BLOCKING 4: expected Gap when after_seq < gc_floor"),
    }

    // Scan from gc_floor (1): both real entries visible.
    match store.scan_log_since_seq(1, 100).unwrap() {
        ScanLogResult::Page(page) => {
            assert_eq!(
                page.entries.len(),
                2,
                "BLOCKING 4: both entries must be reachable via scan from gc_floor"
            );
        }
        ScanLogResult::Gap(_) => panic!("BLOCKING 4: scan from gc_floor must not return Gap"),
    }
}

// ── SHOULD-FIX 6: stable persisted format ────────────────────────────────────

/// Insert an event, read the raw JSON from `nmp-ingest-log`, and verify:
///   - `"version":1` is present
///   - Variant names match the pinned serde renames ("Inserted")
#[test]
fn lmdb_persisted_format_version_and_stable_names() {
    let (store, _dir) = open_tmp();

    let ev = signed_event(1, 1000, "format test", None);
    store
        .insert(verified(ev), &TEST_RELAY.into(), 1_000_000)
        .unwrap();

    let inner = store.inner_for_test();
    let txn = inner.env.read_txn().expect("read_txn");
    let raw = inner
        .ingest_log
        .get(&txn, &1u64.to_be_bytes())
        .expect("get seq=1")
        .expect("entry must exist");
    let json_str = std::str::from_utf8(raw).expect("valid utf8");

    assert!(
        json_str.contains("\"version\":1"),
        "SHOULD-FIX 6: persisted JSON must contain '\"version\":1'; got: {json_str}"
    );
    assert!(
        json_str.contains("\"Inserted\"") || json_str.contains("\"op\":\"Inserted\""),
        "SHOULD-FIX 6: persisted JSON must use stable variant name 'Inserted'; got: {json_str}"
    );

    let value: serde_json::Value = serde_json::from_str(json_str).expect("valid json");
    assert_eq!(value["version"], 1, "version must round-trip as 1");
}
