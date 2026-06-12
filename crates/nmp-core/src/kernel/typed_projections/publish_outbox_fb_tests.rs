//! Round-trip proof for the `publish_outbox` Tier-2 typed codec.

use super::*;

fn sample() -> PublishOutboxModel {
    // ADR-0032 / V-115: `created_at` is raw Unix seconds; `target_summary`
    // removed. Shells format timestamps and compose "N relays · time" themselves.
    PublishOutboxModel {
        items: vec![
            PublishOutboxItemRow {
                handle: "h-1".to_string(),
                event_id: "a".repeat(64),
                kind: 1,
                title: "Note".to_string(),
                preview: "hello world".to_string(),
                created_at: 1_700_000_000,
                status: "sending".to_string(),
                status_label: "Sending".to_string(),
                system_image: "text.bubble".to_string(),
                can_retry: false,
                target_relays: 2,
                relays: vec![
                    PublishOutboxRelayRow {
                        relay_url: "wss://relay.one/".to_string(),
                        status: "sending".to_string(),
                        status_label: "Sending".to_string(),
                        attempt: 0,
                        attempt_label: String::new(),
                        message: "Waiting for relay OK".to_string(),
                        relay_reason: "NIP-65 write relay".to_string(),
                    },
                    PublishOutboxRelayRow {
                        relay_url: "wss://relay.two/".to_string(),
                        status: "retrying".to_string(),
                        status_label: "Retrying".to_string(),
                        attempt: 3,
                        attempt_label: "try 3".to_string(),
                        message: "No response from relay".to_string(),
                        relay_reason: String::new(),
                    },
                ],
            },
            PublishOutboxItemRow {
                handle: "h-2".to_string(),
                event_id: "b".repeat(64),
                kind: 7,
                title: "Reaction".to_string(),
                preview: "Reaction event".to_string(),
                created_at: 1_699_900_000,
                status: "pending".to_string(),
                status_label: "Pending".to_string(),
                system_image: "heart".to_string(),
                can_retry: true,
                target_relays: 0,
                relays: Vec::new(),
            },
        ],
    }
}

#[test]
fn encode_decode_round_trips_and_preserves_order() {
    let model = sample();
    let bytes = encode_publish_outbox(&model);
    let decoded = decode_publish_outbox(&bytes).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve every item + nested relay, in order"
    );
}

#[test]
fn empty_outbox_round_trips() {
    let model = PublishOutboxModel::default();
    let bytes = encode_publish_outbox(&model);
    let decoded = decode_publish_outbox(&bytes).expect("decode must succeed");
    assert!(decoded.items.is_empty());
}

#[test]
fn item_with_no_relays_round_trips() {
    let model = PublishOutboxModel {
        items: vec![PublishOutboxItemRow {
            handle: "h-3".to_string(),
            event_id: "c".repeat(64),
            kind: 1,
            title: "Note".to_string(),
            preview: "p".to_string(),
            created_at: 0,
            status: "queued".to_string(),
            status_label: "Queued".to_string(),
            system_image: "doc.text".to_string(),
            can_retry: true,
            target_relays: 0,
            relays: Vec::new(),
        }],
    };
    let decoded =
        decode_publish_outbox(&encode_publish_outbox(&model)).expect("decode must succeed");
    assert_eq!(decoded, model);
    assert!(decoded.items[0].relays.is_empty());
}

#[test]
fn buffer_carries_the_kpbo_file_identifier() {
    let bytes = encode_publish_outbox(&sample());
    assert_eq!(
        &bytes[4..8],
        PUBLISH_OUTBOX_FILE_IDENTIFIER,
        "the buffer must embed the KPBO file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_publish_outbox(&[]).is_err());
    assert!(decode_publish_outbox(b"NMPU0000").is_err());
}
