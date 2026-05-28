//! Parity tests: typed `nmp.feed.home` sidecar encodes and decodes without
//! losing the projection's semantics (ADR-0037 acceptance criterion).

use nmp_core::{decode_snapshot_with_typed, encode_snapshot_with_typed, TypedProjectionData};
use nmp_nip01::typed_wire::{
    decode_modular_timeline_snapshot, encode_modular_timeline_snapshot, FILE_IDENTIFIER, SCHEMA_ID,
    SCHEMA_VERSION,
};
use nmp_nip01::ModularTimelineSnapshot;

#[test]
fn empty_snapshot_carries_through_envelope() {
    let snapshot = ModularTimelineSnapshot::empty();
    let bytes = encode_modular_timeline_snapshot(&snapshot);
    let typed = vec![TypedProjectionData {
        key: "nmp.feed.home".to_string(),
        schema_id: SCHEMA_ID.to_string(),
        schema_version: SCHEMA_VERSION,
        file_identifier: std::str::from_utf8(FILE_IDENTIFIER).unwrap().to_string(),
        payload: bytes.clone(),
    }];
    let envelope = encode_snapshot_with_typed(serde_json::json!({"rev": 1}), &typed);
    let (_, recovered) = decode_snapshot_with_typed(&envelope).expect("decode");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].key, "nmp.feed.home");
    assert_eq!(recovered[0].schema_id, SCHEMA_ID);
    assert_eq!(recovered[0].payload, bytes);
}

#[test]
fn typed_decode_round_trips_empty_snapshot() {
    let snapshot = ModularTimelineSnapshot::empty();
    let bytes = encode_modular_timeline_snapshot(&snapshot);
    let decoded = decode_modular_timeline_snapshot(&bytes).expect("must decode");
    assert_eq!(decoded.blocks.len(), 0);
    assert_eq!(decoded.cards.len(), 0);
    assert!(decoded.page.is_none());
    assert!(decoded.metrics.is_none());
}

#[test]
fn schema_constants_match_adr_0037() {
    assert_eq!(SCHEMA_ID, "nmp.nip01.timeline");
    assert_eq!(FILE_IDENTIFIER, b"NFTS");
    assert_eq!(SCHEMA_VERSION, 1);
}
