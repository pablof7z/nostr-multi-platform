//! End-to-end proof for the Wave C action-lifecycle + relay-diagnostics-cluster
//! Tier-2 typed projection sidecars (`action_results` / `signed_events` /
//! `action_stages` / `action_lifecycle` / `relay_diagnostics`).
//!
//! These five are the *capture-once* built-ins: their producing accessors drain
//! (`action_results` / `signed_events`), mutate (`action_lifecycle`'s TTL sweep),
//! are mutated mid-tick (`action_stages`), or format wall-clock-relative labels
//! against an internal `now` (`relay_diagnostics`) — so the typed sidecar reads a
//! per-tick `Kernel`-field capture written at the JSON-insertion site, never a
//! second accessor call. This module proves, against the REAL emitted frame:
//!
//! 1. when an action settles, each drain-on-emit built-in's typed sidecar is
//!    present EXACTLY when its generic JSON key is, decodes back to its typed
//!    struct, and agrees with the JSON FIELD-FOR-FIELD (the divergence-safety
//!    invariant — a count check would sail past a dropped field in the
//!    parse-Value path);
//! 2. on the NEXT steady tick the two true drains (`action_results` /
//!    `signed_events`) are ABSENT in BOTH forms — proving the per-tick capture
//!    carries no stale data AND the typed path did not re-drain (no double-drain:
//!    the terminal is not re-surfaced);
//! 3. `relay_diagnostics` is unconditional and its captured struct maps
//!    field-for-field through the typed sidecar.

use super::typed_projections::{
    decode_action_lifecycle, decode_action_results, decode_action_stages, decode_relay_diagnostics,
    decode_signed_events, ACTION_LIFECYCLE_FILE_IDENTIFIER, ACTION_LIFECYCLE_SCHEMA_ID,
    ACTION_LIFECYCLE_SCHEMA_VERSION, ACTION_RESULTS_FILE_IDENTIFIER, ACTION_RESULTS_SCHEMA_ID,
    ACTION_RESULTS_SCHEMA_VERSION, ACTION_STAGES_FILE_IDENTIFIER, ACTION_STAGES_SCHEMA_ID,
    ACTION_STAGES_SCHEMA_VERSION, RELAY_DIAGNOSTICS_FILE_IDENTIFIER, RELAY_DIAGNOSTICS_SCHEMA_ID,
    RELAY_DIAGNOSTICS_SCHEMA_VERSION, SIGNED_EVENTS_FILE_IDENTIFIER, SIGNED_EVENTS_SCHEMA_ID,
    SIGNED_EVENTS_SCHEMA_VERSION,
};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::update_envelope::TypedProjectionData;

fn typed_entry<'a>(typed: &'a [TypedProjectionData], key: &str) -> &'a TypedProjectionData {
    typed
        .iter()
        .find(|t| t.key == key)
        .unwrap_or_else(|| panic!("typed sidecar must carry a `{key}` entry; got {typed:?}"))
}

fn projections_of(value: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value
        .get("projections")
        .and_then(serde_json::Value::as_object)
        .expect("snapshot must carry a projections object")
        .clone()
}

/// `relay_diagnostics` is unconditional: present in BOTH forms even on a fresh
/// kernel, and the captured struct maps field-for-field through the typed
/// sidecar (`relays` / `interests` row counts agree with the JSON).
#[test]
fn relay_diagnostics_emits_typed_sidecar_alongside_json() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (value, typed) = kernel.make_update_typed_for_test(true);
    let projections = projections_of(&value);

    let json_rd = projections
        .get("relay_diagnostics")
        .and_then(serde_json::Value::as_object)
        .expect("the generic JSON `relay_diagnostics` entry must be present (additive)");
    let rd = typed_entry(&typed, "relay_diagnostics");
    assert_eq!(rd.schema_id, RELAY_DIAGNOSTICS_SCHEMA_ID);
    assert_eq!(rd.schema_version, RELAY_DIAGNOSTICS_SCHEMA_VERSION);
    assert_eq!(rd.file_identifier.as_bytes(), RELAY_DIAGNOSTICS_FILE_IDENTIFIER);
    let decoded = decode_relay_diagnostics(&rd.payload).expect("relay_diagnostics must decode");

    let json_relays = json_rd
        .get("relays")
        .and_then(serde_json::Value::as_array)
        .expect("relay_diagnostics JSON must carry a relays array");
    let json_interests = json_rd
        .get("interests")
        .and_then(serde_json::Value::as_array)
        .expect("relay_diagnostics JSON must carry an interests array");
    assert_eq!(
        decoded.relays.len(),
        json_relays.len(),
        "typed and JSON relay_diagnostics.relays must carry the same row count"
    );
    assert_eq!(
        decoded.interests.len(),
        json_interests.len(),
        "typed and JSON relay_diagnostics.interests must carry the same row count"
    );
    // Field-for-field parity on the interest rows the fresh kernel pre-rolls
    // (the diagnostics roll-up seeds default logical-interest rows): proves the
    // struct->Model mapping carries the pre-formatted strings, not just counts.
    for (decoded_i, json_i) in decoded.interests.iter().zip(json_interests.iter()) {
        assert_eq!(
            Some(decoded_i.state.as_str()),
            json_i.get("state").and_then(serde_json::Value::as_str),
            "interest.state must agree field-for-field"
        );
        assert_eq!(
            Some(decoded_i.state_tone.as_str()),
            json_i.get("state_tone").and_then(serde_json::Value::as_str),
            "interest.state_tone must agree field-for-field"
        );
    }
}

/// The drain-on-emit four (`action_results` / `signed_events` / `action_stages` /
/// `action_lifecycle`) are absent on a quiet kernel; once an action settles AND a
/// sign-and-return result lands, each typed sidecar is present EXACTLY when its
/// JSON key is and agrees field-for-field; the next steady tick proves the two
/// true drains vanish in BOTH forms (no stale capture, no double-drain).
#[test]
fn drain_on_emit_builtins_present_iff_json_then_vanish() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);

    // --- quiet kernel: all four absent in BOTH forms -----------------------
    {
        let (value, typed) = kernel.make_update_typed_for_test(true);
        let projections = projections_of(&value);
        for key in [
            "action_results",
            "signed_events",
            "action_stages",
            "action_lifecycle",
        ] {
            assert!(
                !projections.contains_key(key),
                "JSON must omit {key} when nothing settled/tracked"
            );
            assert!(
                !typed.iter().any(|t| t.key == key),
                "typed sidecar must omit {key} when JSON does"
            );
        }
    }

    // --- settle one action + land one sign-and-return ----------------------
    // `record_action_success` writes the `action_results` terminal AND records an
    // `Accepted` stage into both `action_stages` and `action_lifecycle`.
    kernel.record_action_success(
        "corr-pub".to_string(),
        Some(r#"{"event_id":"abcd"}"#.to_string()),
    );
    kernel.record_signed_event_return("corr-sign", Ok(r#"{"id":"sigid","sig":"ff"}"#.to_string()));

    let (value, typed) = kernel.make_update_typed_for_test(true);
    let projections = projections_of(&value);

    // --- action_results: present in BOTH, field-for-field parity -----------
    let json_ar = projections
        .get("action_results")
        .and_then(serde_json::Value::as_array)
        .expect("JSON action_results must be present once an action settled");
    let ar = typed_entry(&typed, "action_results");
    assert_eq!(ar.schema_id, ACTION_RESULTS_SCHEMA_ID);
    assert_eq!(ar.schema_version, ACTION_RESULTS_SCHEMA_VERSION);
    assert_eq!(ar.file_identifier.as_bytes(), ACTION_RESULTS_FILE_IDENTIFIER);
    let decoded_ar = decode_action_results(&ar.payload).expect("action_results must decode");
    assert_eq!(decoded_ar.results.len(), json_ar.len());
    assert_eq!(decoded_ar.results.len(), 1, "exactly the one settled action");
    let row = &decoded_ar.results[0];
    let json_row = &json_ar[0];
    assert_eq!(row.correlation_id, "corr-pub");
    assert_eq!(
        Some(row.correlation_id.as_str()),
        json_row.get("correlation_id").and_then(serde_json::Value::as_str),
        "correlation_id must agree field-for-field"
    );
    assert_eq!(
        Some(row.status.as_str()),
        json_row.get("status").and_then(serde_json::Value::as_str),
        "status must agree field-for-field (record_action_success -> \"published\")"
    );
    assert_eq!(row.status, "published");
    // The opaque `result` body is forwarded verbatim — compare SEMANTICALLY
    // (re-parsed), since the captured string is a re-serialisation of the JSON.
    let typed_result: serde_json::Value =
        serde_json::from_str(row.result.as_ref().expect("result body present")).unwrap();
    assert_eq!(
        typed_result,
        *json_row.get("result").expect("JSON row carries result"),
        "the forwarded result body must agree semantically"
    );

    // --- signed_events: present in BOTH, field-for-field parity ------------
    let json_se = projections
        .get("signed_events")
        .and_then(serde_json::Value::as_object)
        .expect("JSON signed_events must be present once a sign-return landed");
    let se = typed_entry(&typed, "signed_events");
    assert_eq!(se.schema_id, SIGNED_EVENTS_SCHEMA_ID);
    assert_eq!(se.schema_version, SIGNED_EVENTS_SCHEMA_VERSION);
    assert_eq!(se.file_identifier.as_bytes(), SIGNED_EVENTS_FILE_IDENTIFIER);
    let decoded_se = decode_signed_events(&se.payload).expect("signed_events must decode");
    assert_eq!(decoded_se.entries.len(), json_se.len());
    assert_eq!(decoded_se.entries[0].0, "corr-sign");
    let se_row = &decoded_se.entries[0].1;
    let json_se_val = json_se.get("corr-sign").expect("JSON keyed by corr-sign");
    assert!(se_row.ok);
    assert_eq!(
        se_row.ok,
        json_se_val.get("ok").and_then(serde_json::Value::as_bool).unwrap(),
        "signed_events.ok must agree field-for-field"
    );
    assert_eq!(
        se_row.signed_json.as_deref(),
        json_se_val.get("signed_json").and_then(serde_json::Value::as_str),
        "signed_events.signed_json must agree field-for-field"
    );
    assert_eq!(se_row.error, None);

    // --- action_stages: present in BOTH, field-for-field parity ------------
    let json_as = projections
        .get("action_stages")
        .and_then(serde_json::Value::as_object)
        .expect("JSON action_stages must be present once an action is tracked");
    let ast = typed_entry(&typed, "action_stages");
    assert_eq!(ast.schema_id, ACTION_STAGES_SCHEMA_ID);
    assert_eq!(ast.schema_version, ACTION_STAGES_SCHEMA_VERSION);
    assert_eq!(ast.file_identifier.as_bytes(), ACTION_STAGES_FILE_IDENTIFIER);
    let decoded_as = decode_action_stages(&ast.payload).expect("action_stages must decode");
    assert_eq!(decoded_as.entries.len(), json_as.len());
    assert_eq!(decoded_as.entries[0].0, "corr-pub");
    let json_as_history = json_as
        .get("corr-pub")
        .and_then(serde_json::Value::as_array)
        .expect("action_stages history is an array");
    let decoded_history = &decoded_as.entries[0].1;
    assert_eq!(decoded_history.len(), json_as_history.len());
    // The `Accepted` terminal stage carries through as `"accepted"`.
    let last = decoded_history.last().expect("at least one stage");
    let json_last = json_as_history.last().unwrap();
    assert_eq!(
        Some(last.stage.as_str()),
        json_last.get("stage").and_then(serde_json::Value::as_str),
        "action_stages stage must agree field-for-field"
    );
    assert_eq!(last.stage, "accepted");

    // --- action_lifecycle: present in BOTH, field-for-field parity ---------
    let json_al = projections
        .get("action_lifecycle")
        .and_then(serde_json::Value::as_object)
        .expect("JSON action_lifecycle must be present once an action is tracked");
    let al = typed_entry(&typed, "action_lifecycle");
    assert_eq!(al.schema_id, ACTION_LIFECYCLE_SCHEMA_ID);
    assert_eq!(al.schema_version, ACTION_LIFECYCLE_SCHEMA_VERSION);
    assert_eq!(al.file_identifier.as_bytes(), ACTION_LIFECYCLE_FILE_IDENTIFIER);
    let decoded_al = decode_action_lifecycle(&al.payload).expect("action_lifecycle must decode");
    let json_recent = json_al
        .get("recent_terminal")
        .and_then(serde_json::Value::as_array)
        .expect("action_lifecycle JSON carries recent_terminal");
    assert_eq!(decoded_al.recent_terminal.len(), json_recent.len());
    assert_eq!(
        decoded_al.recent_terminal[0].correlation_id, "corr-pub",
        "the settled action appears in recent_terminal"
    );
    assert_eq!(decoded_al.recent_terminal[0].stage, "accepted");

    // --- NEXT steady tick: the two true drains vanish in BOTH forms --------
    // This proves (a) no stale carryover from the per-tick capture and (b) no
    // double-drain: the terminal/sign result is NOT re-surfaced.
    let (value2, typed2) = kernel.make_update_typed_for_test(true);
    let projections2 = projections_of(&value2);
    for key in ["action_results", "signed_events"] {
        assert!(
            !projections2.contains_key(key),
            "{key} JSON must vanish on the next steady tick (drained once)"
        );
        assert!(
            !typed2.iter().any(|t| t.key == key),
            "{key} typed sidecar must vanish too (no stale capture, no re-drain)"
        );
    }
    // `relay_diagnostics` stays present (unconditional) across both ticks.
    assert!(typed2.iter().any(|t| t.key == "relay_diagnostics"));
}
