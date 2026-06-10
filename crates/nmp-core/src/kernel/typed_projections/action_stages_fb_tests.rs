//! Round-trip + JSON-parse proof for the `action_stages` Tier-2 typed codec.

use super::*;

fn sample() -> ActionStagesModel {
    ActionStagesModel {
        entries: vec![(
            "corr-1".to_string(),
            vec![
                ActionStageEntryRow {
                    stage: "publishing".to_string(),
                    reason: None,
                    at_ms: 1_700_000_000_000,
                    detail: Some(r#"{"relay":"wss://r"}"#.to_string()),
                },
                ActionStageEntryRow {
                    stage: "failed".to_string(),
                    reason: Some("no relays".to_string()),
                    at_ms: 1_700_000_000_500,
                    detail: None,
                },
            ],
        )],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded = decode_action_stages(&encode_action_stages(&model)).expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve history order, reason / detail Option presence"
    );
}

#[test]
fn empty_map_round_trips() {
    let model = ActionStagesModel::default();
    let decoded = decode_action_stages(&encode_action_stages(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
}

/// Parse path: mirror the producer's `{correlation_id: [{stage, reason?, at_ms,
/// detail?}]}` shape; entries key-sorted; the `failed` variant's `reason` sibling
/// is lifted.
#[test]
fn model_from_json_mirrors_the_producer_shape() {
    let value = serde_json::json!({
        "corr-z": [ { "stage": "requested", "at_ms": 1 } ],
        "corr-a": [
            { "stage": "publishing", "at_ms": 2, "detail": { "relay": "wss://r" } },
            { "stage": "failed", "reason": "boom", "at_ms": 3 }
        ],
    });
    let model = model_from_json(&value);
    assert_eq!(model.entries.len(), 2);
    assert_eq!(model.entries[0].0, "corr-a", "entries are key-sorted");
    assert_eq!(model.entries[1].0, "corr-z");

    let history = &model.entries[0].1;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].stage, "publishing");
    assert_eq!(history[0].at_ms, 2);
    assert_eq!(history[0].reason, None);
    let detail: serde_json::Value =
        serde_json::from_str(history[0].detail.as_ref().unwrap()).unwrap();
    assert_eq!(detail, serde_json::json!({ "relay": "wss://r" }));

    assert_eq!(history[1].stage, "failed");
    assert_eq!(history[1].reason.as_deref(), Some("boom"));
    assert_eq!(history[1].detail, None, "absent detail key -> None");
}

#[test]
fn buffer_carries_the_kast_file_identifier() {
    let bytes = encode_action_stages(&sample());
    assert_eq!(&bytes[4..8], ACTION_STAGES_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_action_stages(&[]).is_err());
    assert!(decode_action_stages(b"NMPU0000").is_err());
}
