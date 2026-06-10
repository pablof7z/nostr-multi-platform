//! Round-trip proof for the `outbox_summary` Tier-2 typed codec.

use super::*;

fn sample() -> OutboxSummaryModel {
    OutboxSummaryModel {
        title: "3 pending publishes".to_string(),
        subtitle: "1 waiting to retry, 2 currently sending.".to_string(),
        total: 3,
        sending: 2,
        retrying: 1,
        queued: 0,
        failed: 0,
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let bytes = encode_outbox_summary(&model);
    let decoded = decode_outbox_summary(&bytes).expect("decode must succeed");
    assert_eq!(decoded, model, "round-trip must preserve every counter + string");
}

#[test]
fn empty_summary_round_trips() {
    // Mirrors the steady-state `total = 0` summary: the kernel still owns
    // non-empty `title` / `subtitle` strings even when no publish is in flight.
    let model = OutboxSummaryModel {
        title: "Nothing waiting".to_string(),
        subtitle: "Your local outbox is clear.".to_string(),
        ..OutboxSummaryModel::default()
    };
    let decoded = decode_outbox_summary(&encode_outbox_summary(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
    assert_eq!(decoded.total, 0);
}

#[test]
fn buffer_carries_the_koxs_file_identifier() {
    let bytes = encode_outbox_summary(&sample());
    assert_eq!(
        &bytes[4..8],
        OUTBOX_SUMMARY_FILE_IDENTIFIER,
        "the buffer must embed the KOXS file identifier at offset 4..8"
    );
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_outbox_summary(&[]).is_err());
    assert!(decode_outbox_summary(b"NMPU0000").is_err());
}
