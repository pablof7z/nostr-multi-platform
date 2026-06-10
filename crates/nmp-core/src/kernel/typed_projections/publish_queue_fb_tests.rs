//! Round-trip proof for the `publish_queue` Tier-2 typed codec.

use super::*;

fn sample() -> PublishQueueModel {
    PublishQueueModel {
        entries: vec![
            PublishQueueEntryRow {
                event_id: "a".repeat(64),
                kind: 1,
                title: "Note".to_string(),
                target_relays: 2,
                status: "ok".to_string(),
                can_retry: false,
                relay_outcomes: vec![
                    RelayAckOutcomeRow {
                        relay_url: "wss://relay.one/".to_string(),
                        status: "ok".to_string(),
                        message: String::new(),
                        relay_reason: "NIP-65 write relay".to_string(),
                    },
                    RelayAckOutcomeRow {
                        relay_url: "wss://relay.two/".to_string(),
                        status: "failed".to_string(),
                        message: "gave up after 5 retries".to_string(),
                        relay_reason: String::new(),
                    },
                ],
            },
            PublishQueueEntryRow {
                event_id: "b".repeat(64),
                kind: 7,
                title: "Reaction".to_string(),
                target_relays: 0,
                status: "accepted_locally".to_string(),
                can_retry: true,
                relay_outcomes: Vec::new(),
            },
        ],
    }
}

#[test]
fn encode_decode_round_trips_and_preserves_order() {
    let model = sample();
    let bytes = encode_publish_queue(&model);
    let decoded = decode_publish_queue(&bytes).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve every entry + nested outcome, in order"
    );
}

#[test]
fn empty_queue_round_trips() {
    let model = PublishQueueModel::default();
    let bytes = encode_publish_queue(&model);
    let decoded = decode_publish_queue(&bytes).expect("decode must succeed");
    assert!(decoded.entries.is_empty());
}

#[test]
fn entry_with_no_outcomes_round_trips() {
    // Mirrors an `accepted_locally` row: empty `relay_outcomes` (the JSON path
    // skips the key entirely; the typed buffer carries an empty vector).
    let model = PublishQueueModel {
        entries: vec![PublishQueueEntryRow {
            event_id: "c".repeat(64),
            kind: 1,
            title: "Note".to_string(),
            target_relays: 3,
            status: "accepted_locally".to_string(),
            can_retry: true,
            relay_outcomes: Vec::new(),
        }],
    };
    let decoded = decode_publish_queue(&encode_publish_queue(&model)).expect("decode must succeed");
    assert_eq!(decoded, model);
    assert!(decoded.entries[0].relay_outcomes.is_empty());
}

#[test]
fn buffer_carries_the_kpbq_file_identifier() {
    let bytes = encode_publish_queue(&sample());
    assert_eq!(
        &bytes[4..8],
        PUBLISH_QUEUE_FILE_IDENTIFIER,
        "the buffer must embed the KPBQ file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_publish_queue(&[]).is_err());
    assert!(decode_publish_queue(b"NMPU0000").is_err());
}
