//! Round-trip tests for the [`ZapsAggregateSnapshot`] typed FlatBuffers codec.

use super::{
    decode_zaps_snapshot, encode_zaps_snapshot, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION,
};
use crate::projection::{ZapCount, ZapsAggregateSnapshot};

fn populated_snapshot() -> ZapsAggregateSnapshot {
    let mut snapshot = ZapsAggregateSnapshot::empty();
    snapshot.totals.insert(
        "a".repeat(64),
        ZapCount {
            total_msats: 21_000,
            count: 3,
        },
    );
    snapshot.totals.insert(
        "b".repeat(64),
        ZapCount {
            total_msats: 0,
            count: 1,
        },
    );
    snapshot
}

#[test]
fn round_trips_populated_snapshot() {
    let snapshot = populated_snapshot();
    let bytes = encode_zaps_snapshot(&snapshot);
    let decoded = decode_zaps_snapshot(&bytes).expect("decode must succeed");
    assert_eq!(decoded, snapshot);
}

#[test]
fn round_trips_empty_snapshot() {
    let snapshot = ZapsAggregateSnapshot::empty();
    let bytes = encode_zaps_snapshot(&snapshot);
    let decoded = decode_zaps_snapshot(&bytes).expect("decode must succeed");
    assert_eq!(decoded, snapshot);
    assert!(decoded.totals.is_empty());
}

#[test]
fn preserves_count_distinct_from_msats() {
    // A receipt with an unparseable amount contributes 0 msats but still
    // increments `count` — both fields must survive the round-trip distinctly.
    let mut snapshot = ZapsAggregateSnapshot::empty();
    snapshot.totals.insert(
        "c".repeat(64),
        ZapCount {
            total_msats: 0,
            count: 5,
        },
    );
    let bytes = encode_zaps_snapshot(&snapshot);
    let decoded = decode_zaps_snapshot(&bytes).expect("decode must succeed");
    let entry = decoded.totals.get(&"c".repeat(64)).expect("entry present");
    assert_eq!(entry.total_msats, 0);
    assert_eq!(entry.count, 5);
}

#[test]
fn encoding_is_deterministic_across_iteration_order() {
    // Two snapshots with identical content must encode to identical bytes
    // regardless of HashMap insertion order (totals are sorted on the wire).
    let mut a = ZapsAggregateSnapshot::empty();
    a.totals.insert("11".repeat(32), ZapCount { total_msats: 1, count: 1 });
    a.totals.insert("22".repeat(32), ZapCount { total_msats: 2, count: 2 });

    let mut b = ZapsAggregateSnapshot::empty();
    b.totals.insert("22".repeat(32), ZapCount { total_msats: 2, count: 2 });
    b.totals.insert("11".repeat(32), ZapCount { total_msats: 1, count: 1 });

    assert_eq!(encode_zaps_snapshot(&a), encode_zaps_snapshot(&b));
}

#[test]
fn encoded_buffer_carries_the_nzap_file_identifier() {
    let bytes = encode_zaps_snapshot(&populated_snapshot());
    assert!(super::generated::nmp::nip_57::zaps_snapshot_buffer_has_identifier(&bytes));
    assert_eq!(FILE_IDENTIFIER, b"NZAP");
}

#[test]
fn decode_rejects_buffer_without_identifier() {
    assert!(decode_zaps_snapshot(&[]).is_err());
    assert!(decode_zaps_snapshot(b"not a flatbuffer at all").is_err());
}

#[test]
fn schema_constants_match_the_fbs() {
    assert_eq!(SCHEMA_ID, "nmp.nip57.zaps");
    assert_eq!(SCHEMA_VERSION, 1);
}
