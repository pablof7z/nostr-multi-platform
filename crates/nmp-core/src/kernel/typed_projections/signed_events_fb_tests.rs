//! Round-trip + JSON-parse proof for the `signed_events` Tier-2 typed codec.

use super::*;

fn sample() -> SignedEventsModel {
    SignedEventsModel {
        entries: vec![
            (
                "corr-a".to_string(),
                SignedEventRow {
                    correlation_id: "corr-a".to_string(),
                    ok: true,
                    signed_json: Some(r#"{"id":"abcd","sig":"ff"}"#.to_string()),
                    error: None,
                },
            ),
            (
                "corr-b".to_string(),
                SignedEventRow {
                    correlation_id: "corr-b".to_string(),
                    ok: false,
                    signed_json: None,
                    error: Some("signer rejected".to_string()),
                },
            ),
        ],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded = decode_signed_events(&encode_signed_events(&model)).expect("decode must succeed");
    assert_eq!(decoded, model);
}

#[test]
fn empty_map_round_trips() {
    let model = SignedEventsModel::default();
    let decoded = decode_signed_events(&encode_signed_events(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
}

/// Parse path: mirror the producer's `{correlation_id: {ok, signed_json|error}}`
/// shape, and confirm entries come out key-sorted (the producer is BTree-ordered).
#[test]
fn model_from_json_mirrors_the_producer_shape_sorted() {
    let value = serde_json::json!({
        "corr-b": { "ok": false, "error": "signer rejected" },
        "corr-a": { "ok": true, "signed_json": "{\"id\":\"abcd\"}" },
    });
    let model = model_from_json(&value);
    assert_eq!(model.entries.len(), 2);
    // Key-sorted regardless of insertion order.
    assert_eq!(model.entries[0].0, "corr-a");
    assert_eq!(model.entries[1].0, "corr-b");

    assert!(model.entries[0].1.ok);
    assert_eq!(
        model.entries[0].1.signed_json.as_deref(),
        Some("{\"id\":\"abcd\"}")
    );
    assert_eq!(model.entries[0].1.error, None);

    assert!(!model.entries[1].1.ok);
    assert_eq!(model.entries[1].1.signed_json, None);
    assert_eq!(model.entries[1].1.error.as_deref(), Some("signer rejected"));
    // The correlation_id is stamped from the map key.
    assert_eq!(model.entries[1].1.correlation_id, "corr-b");
}

#[test]
fn model_from_json_degrades_on_non_object() {
    assert!(model_from_json(&serde_json::Value::Null).entries.is_empty());
}

#[test]
fn buffer_carries_the_ksev_file_identifier() {
    let bytes = encode_signed_events(&sample());
    assert_eq!(&bytes[4..8], SIGNED_EVENTS_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_signed_events(&[]).is_err());
    assert!(decode_signed_events(b"NMPU0000").is_err());
}
