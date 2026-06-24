//! Round-trip tests for the `N50S` typed search-results codec.

use super::*;
use crate::{SearchHit, SearchHitSource, SearchResultsSnapshot};

fn sample() -> SearchResultsSnapshot {
    SearchResultsSnapshot {
        hits: vec![
            SearchHit {
                id: "e1".to_string(),
                author: "aa".to_string(),
                kind: 1,
                created_at: 200,
                content: "hello nostr".to_string(),
                tags: vec![vec!["t".to_string(), "nostr".to_string()]],
                relay_provenance: vec!["wss://search-relay.example/".to_string()],
                source: SearchHitSource::Relay("wss://search-relay.example/".to_string()),
            },
            SearchHit {
                id: "e2".to_string(),
                author: "bb".to_string(),
                kind: 30023,
                created_at: 100,
                content: "an article".to_string(),
                tags: Vec::new(),
                relay_provenance: Vec::new(),
                source: SearchHitSource::Cache,
            },
        ],
    }
}

#[test]
fn round_trips_through_n50s_buffer() {
    let snap = sample();
    let bytes = encode_search_results_snapshot(&snap);
    assert!(bytes.len() >= 8);
    let decoded = decode_search_results_snapshot(&bytes).expect("decode");
    assert_eq!(decoded, snap);
}

#[test]
fn rejects_buffer_without_identifier() {
    assert!(decode_search_results_snapshot(&[0u8; 4]).is_err());
    assert!(decode_search_results_snapshot(b"not a buffer at all").is_err());
}

#[test]
fn empty_snapshot_round_trips() {
    let snap = SearchResultsSnapshot::default();
    let bytes = encode_search_results_snapshot(&snap);
    let decoded = decode_search_results_snapshot(&bytes).expect("decode");
    assert_eq!(decoded, snap);
}

#[test]
fn cache_vs_relay_provenance_is_preserved() {
    let snap = sample();
    let decoded = decode_search_results_snapshot(&encode_search_results_snapshot(&snap)).unwrap();
    assert_eq!(
        decoded.hits[0].source,
        SearchHitSource::Relay("wss://search-relay.example/".to_string())
    );
    assert_eq!(decoded.hits[1].source, SearchHitSource::Cache);
}
