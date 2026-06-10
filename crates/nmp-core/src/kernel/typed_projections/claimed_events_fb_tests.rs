//! Round-trip proof for the `claimed_events` Tier-2 typed codec.

use super::*;

fn sample() -> ClaimedEventsModel {
    ClaimedEventsModel {
        entries: vec![
            (
                "ee".repeat(32),
                ClaimedEventRow {
                    primary_id: "ee".repeat(32),
                    id: "ee".repeat(32),
                    author_pubkey: "aa".repeat(32),
                    author_display_name: Some("Alice".to_string()),
                    author_picture_url: Some("https://example.com/a.png".to_string()),
                    kind: 1,
                    created_at: 1_700_000_000,
                    tags: vec![
                        vec!["e".to_string(), "ff".repeat(32), "wss://relay".to_string()],
                        vec!["p".to_string(), "bb".repeat(32)],
                        // An empty inner tag row must survive the round-trip.
                        vec![],
                    ],
                    content: "hello world".to_string(),
                },
            ),
            (
                // naddr coordinate key — author profile not yet ingested.
                "30023:aa:slug".to_string(),
                ClaimedEventRow {
                    primary_id: "30023:aa:slug".to_string(),
                    id: "cc".repeat(32),
                    author_pubkey: "aa".repeat(32),
                    author_display_name: None,
                    author_picture_url: None,
                    kind: 30023,
                    created_at: 1_700_000_500,
                    tags: vec![vec!["d".to_string(), "slug".to_string()]],
                    content: "# Article".to_string(),
                },
            ),
        ],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded =
        decode_claimed_events(&encode_claimed_events(&model)).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve nested tags, key order, kind, and Option presence"
    );
}

#[test]
fn empty_map_round_trips() {
    let model = ClaimedEventsModel::default();
    let decoded = decode_claimed_events(&encode_claimed_events(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
    assert!(decoded.entries.is_empty());
}

#[test]
fn nested_tags_and_none_authors_preserved() {
    let model = ClaimedEventsModel {
        entries: vec![(
            "dd".repeat(32),
            ClaimedEventRow {
                primary_id: "dd".repeat(32),
                id: "dd".repeat(32),
                author_pubkey: "bb".repeat(32),
                author_display_name: None,
                author_picture_url: None,
                kind: 6,
                created_at: 42,
                tags: vec![vec![], vec!["single".to_string()]],
                content: String::new(),
            },
        )],
    };
    let decoded = decode_claimed_events(&encode_claimed_events(&model)).expect("decode succeeds");
    let row = &decoded.entries[0].1;
    assert_eq!(row.author_display_name, None);
    assert_eq!(row.author_picture_url, None);
    assert_eq!(
        row.tags,
        vec![Vec::<String>::new(), vec!["single".to_string()]]
    );
}

#[test]
fn buffer_carries_the_kcev_file_identifier() {
    let bytes = encode_claimed_events(&sample());
    assert_eq!(
        &bytes[4..8],
        CLAIMED_EVENTS_FILE_IDENTIFIER,
        "the buffer must embed the KCEV file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_claimed_events(&[]).is_err());
    assert!(decode_claimed_events(b"NMPU0000").is_err());
}
