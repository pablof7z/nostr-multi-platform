//! Round-trip tests for the [`FollowListSnapshot`] typed FlatBuffers codec.

use super::{decode_follow_list, encode_follow_list, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION};
use crate::projection::{FollowEntry, FollowListSnapshot};

fn populated_snapshot() -> FollowListSnapshot {
    FollowListSnapshot {
        follows: vec![
            FollowEntry {
                pubkey: "a".repeat(64),
            },
            FollowEntry {
                pubkey: "b".repeat(64),
            },
            FollowEntry {
                pubkey: "c".repeat(64),
            },
        ],
    }
}

#[test]
fn round_trips_populated_snapshot() {
    let snapshot = populated_snapshot();
    let bytes = encode_follow_list(&snapshot);
    let decoded = decode_follow_list(&bytes).expect("decode must succeed");
    assert_eq!(decoded, snapshot);
}

#[test]
fn round_trips_empty_snapshot() {
    let snapshot = FollowListSnapshot::default();
    let bytes = encode_follow_list(&snapshot);
    let decoded = decode_follow_list(&bytes).expect("decode must succeed");
    assert_eq!(decoded, snapshot);
    assert!(decoded.follows.is_empty());
}

#[test]
fn preserves_follow_order() {
    // The follow list is an ordered vector; encode/decode must not reorder it.
    let snapshot = populated_snapshot();
    let bytes = encode_follow_list(&snapshot);
    let decoded = decode_follow_list(&bytes).expect("decode must succeed");
    let pubkeys: Vec<&str> = decoded.follows.iter().map(|f| f.pubkey.as_str()).collect();
    assert_eq!(
        pubkeys,
        vec!["a".repeat(64), "b".repeat(64), "c".repeat(64)]
    );
}

#[test]
fn encoded_buffer_carries_the_nf02_file_identifier() {
    let bytes = encode_follow_list(&populated_snapshot());
    assert!(super::generated::nmp::nip_02::follow_list_snapshot_buffer_has_identifier(&bytes));
    assert_eq!(FILE_IDENTIFIER, b"NF02");
}

#[test]
fn decode_rejects_buffer_without_identifier() {
    assert!(decode_follow_list(&[]).is_err());
    assert!(decode_follow_list(b"not a flatbuffer at all").is_err());
}

#[test]
fn schema_constants_match_the_fbs() {
    assert_eq!(SCHEMA_ID, "nmp.nip02.follow_list");
    assert_eq!(SCHEMA_VERSION, 1);
}
