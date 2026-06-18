//! Round-trip proof for the `outbox_summary` Tier-2 typed codec.
//!
//! ADR-0032 / doctrine §4.4: `title` / `subtitle` pre-formatted English
//! strings removed from the wire; only raw counters remain.

use super::*;

fn sample() -> OutboxSummaryModel {
    OutboxSummaryModel {
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
    assert_eq!(decoded, model, "round-trip must preserve every counter");
}

#[test]
fn empty_summary_round_trips() {
    // Steady-state `total = 0` summary: no in-flight publishes.
    let model = OutboxSummaryModel {
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
