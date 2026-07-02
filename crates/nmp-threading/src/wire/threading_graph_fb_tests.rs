use super::*;
use crate::projection::ThreadEdge;
use crate::{ThreadPointer, TimelineBlock};

fn sample() -> ThreadingSnapshot {
    ThreadingSnapshot {
        edges: vec![
            ThreadEdge {
                event_id: "a".repeat(64),
                author_pubkey: "1".repeat(64),
                kind: 1,
                created_at: 100,
                parent: None,
                root: None,
                parent_author_pubkey: None,
            },
            ThreadEdge {
                event_id: "b".repeat(64),
                author_pubkey: "2".repeat(64),
                kind: 1,
                created_at: 200,
                parent: Some(ThreadPointer::Event {
                    id: "a".repeat(64),
                    relay: Some("wss://relay.example".to_string()),
                    kind: Some(1),
                }),
                root: Some(ThreadPointer::Address {
                    coord: "30023:pk:d".to_string(),
                    relay: None,
                    kind: Some(30023),
                }),
                parent_author_pubkey: Some("1".repeat(64)),
            },
        ],
        blocks: vec![
            TimelineBlock::Standalone {
                id: "c".repeat(64),
                root: Some(ThreadPointer::External {
                    uri: "https://example.com/thread".to_string(),
                }),
            },
            TimelineBlock::Module {
                events: vec!["a".repeat(64), "b".repeat(64)],
                has_gap: true,
                root: None,
            },
        ],
        pending_ancestor_ids: vec!["d".repeat(64)],
    }
}

#[test]
fn round_trips_edges_blocks_and_pointers() {
    let snapshot = sample();
    let bytes = encode_threading_snapshot(&snapshot);
    let decoded = decode_threading_snapshot(&bytes).expect("decode");
    assert_eq!(decoded, snapshot);
}

#[test]
fn buffer_carries_nthr_identifier() {
    let bytes = encode_threading_snapshot(&sample());
    assert_eq!(&bytes[4..8], THREADING_GRAPH_FILE_IDENTIFIER);
    assert_eq!(THREADING_GRAPH_SCHEMA_ID, "nmp.threading.graph");
}

#[test]
fn empty_snapshot_round_trips() {
    let snapshot = ThreadingSnapshot::empty();
    let bytes = encode_threading_snapshot(&snapshot);
    assert_eq!(decode_threading_snapshot(&bytes).unwrap(), snapshot);
}

#[test]
fn rejects_foreign_identifier() {
    assert!(decode_threading_snapshot(b"not-a-buffer").is_err());
}
