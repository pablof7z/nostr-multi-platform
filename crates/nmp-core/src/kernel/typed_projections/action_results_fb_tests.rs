//! Round-trip + JSON-parse proof for the `action_results` Tier-2 typed codec.

use super::*;

fn sample() -> ActionResultsModel {
    ActionResultsModel {
        results: vec![
            ActionResultRow {
                correlation_id: "corr-1".to_string(),
                status: "published".to_string(),
                error: None,
                result: Some(r#"{"event_id":"abcd"}"#.to_string()),
            },
            ActionResultRow {
                correlation_id: "corr-2".to_string(),
                status: "failed".to_string(),
                error: Some("no relays".to_string()),
                result: None,
            },
        ],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded =
        decode_action_results(&encode_action_results(&model)).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve order, status, error / result Option presence"
    );
}

#[test]
fn empty_array_round_trips() {
    let model = ActionResultsModel::default();
    let decoded = decode_action_results(&encode_action_results(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
    assert!(decoded.results.is_empty());
}

/// The codec's deviation from #1031: the Model is built by PARSING the captured
/// `serde_json::Value`. Prove the parse mirrors the producer's exact JSON shape
/// (the `take_action_results_projection` row: `{correlation_id, status, error,
/// result?}`) field-for-field.
#[test]
fn model_from_json_mirrors_the_producer_shape() {
    let value = serde_json::json!([
        { "correlation_id": "corr-1", "status": "published", "error": null,
          "result": { "event_id": "abcd" } },
        { "correlation_id": "corr-2", "status": "failed", "error": "no relays" },
    ]);
    let model = model_from_json(&value);
    assert_eq!(model.results.len(), 2);

    assert_eq!(model.results[0].correlation_id, "corr-1");
    assert_eq!(model.results[0].status, "published");
    assert_eq!(model.results[0].error, None, "JSON null -> None");
    // The opaque result body is carried as its serialised JSON string.
    let parsed: serde_json::Value =
        serde_json::from_str(model.results[0].result.as_ref().unwrap()).unwrap();
    assert_eq!(parsed, serde_json::json!({ "event_id": "abcd" }));

    assert_eq!(model.results[1].error.as_deref(), Some("no relays"));
    assert_eq!(model.results[1].result, None, "absent result key -> None");
}

#[test]
fn model_from_json_degrades_on_non_array() {
    assert!(model_from_json(&serde_json::Value::Null).results.is_empty());
    assert!(model_from_json(&serde_json::json!({})).results.is_empty());
}

#[test]
fn buffer_carries_the_kars_file_identifier() {
    let bytes = encode_action_results(&sample());
    assert_eq!(&bytes[4..8], ACTION_RESULTS_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_action_results(&[]).is_err());
    assert!(decode_action_results(b"NMPU0000").is_err());
}
