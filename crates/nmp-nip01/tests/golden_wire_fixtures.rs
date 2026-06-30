//! Golden wire fixtures for [`nmp_nip01::ModularTimelineSnapshot`].

use nmp_nip01::typed_wire::{
    decode_modular_timeline_snapshot, encode_modular_timeline_snapshot, FILE_IDENTIFIER, SCHEMA_ID,
    SCHEMA_VERSION,
};
use nmp_nip01::ModularTimelineSnapshot;
use nmp_threading::{ThreadPointer, TimelineBlock};

fn golden_snapshot() -> ModularTimelineSnapshot {
    ModularTimelineSnapshot::empty()
}

fn golden_block_snapshot() -> ModularTimelineSnapshot {
    ModularTimelineSnapshot {
        blocks: vec![TimelineBlock::Standalone {
            id: "event".to_string(),
            root: Some(ThreadPointer::Event {
                id: "root".to_string(),
                relay: Some("wss://relay.example".to_string()),
                kind: Some(1),
            }),
        }],
    }
}

fn decode_hex(hex: &str) -> Vec<u8> {
    let compact: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(compact.len() % 2, 0, "hex fixture must contain full bytes");
    compact
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("fixture is ascii hex");
            u8::from_str_radix(pair, 16).expect("fixture is valid hex")
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn timeline_snapshot_empty_golden_fixture_is_stable() {
    let wire = encode_modular_timeline_snapshot(&golden_snapshot());
    let expected = decode_hex(include_str!("fixtures/timeline_snapshot_empty_v3.fb.hex"));
    if wire != expected {
        eprintln!(
            "actual timeline_snapshot_empty_v3 hex:\n{}",
            encode_hex(&wire)
        );
    }
    assert_eq!(wire, expected);
}

#[test]
fn timeline_snapshot_with_block_golden_fixture_is_stable() {
    let wire = encode_modular_timeline_snapshot(&golden_block_snapshot());
    let expected = decode_hex(include_str!(
        "fixtures/timeline_snapshot_with_block_v3.fb.hex"
    ));
    if wire != expected {
        eprintln!(
            "actual timeline_snapshot_with_block_v3 hex:\n{}",
            encode_hex(&wire)
        );
    }
    assert_eq!(wire, expected);
}

#[test]
fn timeline_snapshot_golden_fixture_has_nfts_identifier() {
    let wire = encode_modular_timeline_snapshot(&golden_snapshot());
    assert_eq!(&wire[4..8], FILE_IDENTIFIER);
    assert_eq!(FILE_IDENTIFIER, b"NFTS");
}

#[test]
fn schema_id_is_stable() {
    assert_eq!(SCHEMA_ID, "nmp.nip01.timeline");
    assert_eq!(SCHEMA_VERSION, 3);
}

#[test]
fn typed_snapshot_schema_round_trips() {
    let snapshot = golden_block_snapshot();
    let typed_bytes = encode_modular_timeline_snapshot(&snapshot);
    let decoded = decode_modular_timeline_snapshot(&typed_bytes).expect("must decode");
    assert_eq!(decoded, snapshot);
}
