use nmp_threading::{ThreadPointer, TimelineBlock};

use super::*;
use crate::ModularTimelineSnapshot;

fn snapshot_with_blocks() -> ModularTimelineSnapshot {
    ModularTimelineSnapshot {
        blocks: vec![
            TimelineBlock::Standalone {
                id: "standalone".to_string(),
                root: Some(ThreadPointer::Event {
                    id: "root".to_string(),
                    relay: Some("wss://relay.example".to_string()),
                    kind: Some(1),
                }),
            },
            TimelineBlock::Module {
                events: vec!["root".to_string(), "reply".to_string()],
                has_gap: false,
                root: None,
            },
        ],
    }
}

#[test]
fn empty_snapshot_round_trips() {
    let snapshot = ModularTimelineSnapshot::empty();
    let decoded =
        decode_modular_timeline_snapshot(&encode_modular_timeline_snapshot(&snapshot)).unwrap();
    assert_eq!(decoded, snapshot);
}

#[test]
fn block_snapshot_round_trips() {
    let snapshot = snapshot_with_blocks();
    let decoded =
        decode_modular_timeline_snapshot(&encode_modular_timeline_snapshot(&snapshot)).unwrap();
    assert_eq!(decoded, snapshot);
}

#[test]
fn encoded_buffer_has_nfts_identifier() {
    let encoded = encode_modular_timeline_snapshot(&ModularTimelineSnapshot::empty());
    assert!(fb::modular_timeline_snapshot_buffer_has_identifier(
        &encoded
    ));
    assert_eq!(&encoded[4..8], FILE_IDENTIFIER);
}

#[test]
fn rejects_missing_identifier() {
    let err = decode_modular_timeline_snapshot(&[0u8; 16]).expect_err("must reject");
    assert!(err.contains("NFTS"), "error names the missing id: {err}");
}

#[test]
fn schema_constants_are_stable() {
    assert_eq!(SCHEMA_ID, "nmp.nip01.timeline");
    assert_eq!(FILE_IDENTIFIER, b"NFTS");
    assert_eq!(SCHEMA_VERSION, 3);
}
