//! Round-trip + envelope tests for the `mute_list` typed FlatBuffers codec.

use super::*;

#[test]
fn round_trips_populated() {
    let snapshot = MuteListSnapshot {
        muted_pubkeys: vec!["a".repeat(64), "b".repeat(64)],
        muted_event_ids: vec!["c".repeat(64)],
    };
    let bytes = encode_mute_list(&snapshot);
    let decoded = decode_mute_list(&bytes).expect("decode");
    assert_eq!(decoded, snapshot);
}

#[test]
fn empty_round_trips() {
    let snapshot = MuteListSnapshot::default();
    let bytes = encode_mute_list(&snapshot);
    let decoded = decode_mute_list(&bytes).expect("decode");
    assert!(decoded.muted_pubkeys.is_empty());
    assert!(decoded.muted_event_ids.is_empty());
}

#[test]
fn order_preserved() {
    let snapshot = MuteListSnapshot {
        muted_pubkeys: vec!["z".repeat(64), "a".repeat(64), "m".repeat(64)],
        muted_event_ids: vec!["9".repeat(64), "1".repeat(64)],
    };
    let bytes = encode_mute_list(&snapshot);
    let decoded = decode_mute_list(&bytes).expect("decode");
    assert_eq!(decoded.muted_pubkeys, snapshot.muted_pubkeys);
    assert_eq!(decoded.muted_event_ids, snapshot.muted_event_ids);
}

#[test]
fn buffer_carries_nmut_identifier() {
    let bytes = encode_mute_list(&MuteListSnapshot::default());
    assert_eq!(&bytes[4..8], MUTE_LIST_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_garbage() {
    assert!(decode_mute_list(&[0u8; 4]).is_err());
    assert!(decode_mute_list(b"not a flatbuffer").is_err());
}

#[test]
fn schema_consts_are_stable() {
    assert_eq!(MUTE_LIST_SCHEMA_ID, "nmp.nip51.mute_list");
    assert_eq!(MUTE_LIST_FILE_IDENTIFIER, b"NMUT");
    assert_eq!(MUTE_LIST_SCHEMA_VERSION, 1);
}
