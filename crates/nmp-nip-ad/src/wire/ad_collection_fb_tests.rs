//! Round-trip proofs for the `ADCL` typed collection wire codec.

use super::{decode_ad_collection_snapshot, encode_ad_collection_snapshot};
use crate::{AdCollectionRow, AdCollectionSnapshot};

fn row(id: &str, kind: u32, created_at: u64) -> AdCollectionRow {
    AdCollectionRow {
        id: id.to_string(),
        author: "pk".to_string(),
        kind,
        created_at,
        content: format!("content-{id}"),
        tags: vec![
            vec!["d".to_string(), "legible".to_string()],
            vec!["title".to_string(), "The Machine".to_string()],
        ],
        relay_provenance: vec!["wss://relay.primal.net".to_string()],
    }
}

#[test]
fn encode_decode_round_trips_field_for_field() {
    let snapshot = AdCollectionSnapshot {
        rows: vec![row("article-2", 30023, 2000), row("article-1", 30023, 1000)],
    };
    let bytes = encode_ad_collection_snapshot(&snapshot);
    let decoded = decode_ad_collection_snapshot(&bytes).expect("valid ADCL buffer");
    assert_eq!(decoded, snapshot);
}

#[test]
fn empty_snapshot_round_trips() {
    let snapshot = AdCollectionSnapshot::default();
    let bytes = encode_ad_collection_snapshot(&snapshot);
    let decoded = decode_ad_collection_snapshot(&bytes).expect("valid empty ADCL buffer");
    assert!(decoded.rows.is_empty());
}

#[test]
fn decode_rejects_garbage_without_panicking() {
    assert!(decode_ad_collection_snapshot(b"").is_err());
    assert!(decode_ad_collection_snapshot(b"not a flatbuffer").is_err());
    // Right length, wrong file identifier.
    assert!(decode_ad_collection_snapshot(&[0u8; 32]).is_err());
}
