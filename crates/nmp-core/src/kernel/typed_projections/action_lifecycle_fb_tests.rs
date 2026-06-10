//! Round-trip + JSON-parse proof for the `action_lifecycle` Tier-2 typed codec.

use super::*;

fn sample() -> ActionLifecycleModel {
    ActionLifecycleModel {
        in_flight: vec![LifecycleEntryRow {
            correlation_id: "corr-1".to_string(),
            stage: "publishing".to_string(),
            reason: None,
        }],
        recent_terminal: vec![LifecycleEntryRow {
            correlation_id: "corr-2".to_string(),
            stage: "failed".to_string(),
            reason: Some("no relays".to_string()),
        }],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded =
        decode_action_lifecycle(&encode_action_lifecycle(&model)).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve both arrays + reason Option presence"
    );
}

#[test]
fn empty_struct_round_trips() {
    let model = ActionLifecycleModel::default();
    let decoded =
        decode_action_lifecycle(&encode_action_lifecycle(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
    assert!(decoded.in_flight.is_empty());
    assert!(decoded.recent_terminal.is_empty());
}

/// Parse path: mirror the producer's `{in_flight: [...], recent_terminal: [...]}`
/// shape; the `failed` variant's `reason` sibling is lifted.
#[test]
fn model_from_json_mirrors_the_producer_shape() {
    let value = serde_json::json!({
        "in_flight": [ { "correlation_id": "corr-1", "stage": "publishing" } ],
        "recent_terminal": [ { "correlation_id": "corr-2", "stage": "failed", "reason": "boom" } ],
    });
    let model = model_from_json(&value);
    assert_eq!(model.in_flight.len(), 1);
    assert_eq!(model.in_flight[0].correlation_id, "corr-1");
    assert_eq!(model.in_flight[0].stage, "publishing");
    assert_eq!(model.in_flight[0].reason, None);

    assert_eq!(model.recent_terminal.len(), 1);
    assert_eq!(model.recent_terminal[0].stage, "failed");
    assert_eq!(model.recent_terminal[0].reason.as_deref(), Some("boom"));
}

#[test]
fn model_from_json_degrades_on_missing_arrays() {
    let model = model_from_json(&serde_json::json!({}));
    assert!(model.in_flight.is_empty());
    assert!(model.recent_terminal.is_empty());
}

#[test]
fn buffer_carries_the_kalc_file_identifier() {
    let bytes = encode_action_lifecycle(&sample());
    assert_eq!(&bytes[4..8], ACTION_LIFECYCLE_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_action_lifecycle(&[]).is_err());
    assert!(decode_action_lifecycle(b"NMPU0000").is_err());
}
