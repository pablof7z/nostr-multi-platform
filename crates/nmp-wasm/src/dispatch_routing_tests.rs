//! Tests for `dispatch_routing` — ADR-0063 reference-resolution parsing and the
//! stable reason strings. (#1740 step 8 retired the raw interest-dispatch arm.)
//!
//! Moved out of `dispatch_routing.rs` (NMP #169-style file-size split) to
//! keep the module root under the 500-LOC hard ceiling.

use super::*;

#[test]
fn ref_dispatch_routes_resolve_profile() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.resolve_ref".to_string(),
        payload: serde_json::json!({
            "namespace": 0, "key": "abc123", "consumer_id": "chirp-web-author-1",
            "shape": 0, "liveness": 0,
        }),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_action(&action),
        Some(RefDispatch::Resolve {
            namespace: RefNamespace::Profile,
            key: "abc123".to_string(),
            consumer_id: "chirp-web-author-1".to_string(),
            shape: RefShape::Profile(ProfileShape::Ref),
            liveness: RefLiveness::CacheOk,
        })
    );
}

#[test]
fn ref_dispatch_routes_resolve_profile_card_live() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.resolve_ref".to_string(),
        payload: serde_json::json!({
            "namespace": 0, "key": "abc123", "consumer_id": "screen",
            "shape": 1, "liveness": 1,
        }),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_action(&action),
        Some(RefDispatch::Resolve {
            namespace: RefNamespace::Profile,
            key: "abc123".to_string(),
            consumer_id: "screen".to_string(),
            shape: RefShape::Profile(ProfileShape::Card),
            liveness: RefLiveness::Live,
        })
    );
}

#[test]
fn ref_dispatch_routes_resolve_event() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.resolve_ref".to_string(),
        payload: serde_json::json!({
            "namespace": 1, "key": "deadbeef", "consumer_id": "embed-1",
            "shape": 0, "liveness": 0,
        }),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_action(&action),
        Some(RefDispatch::Resolve {
            namespace: RefNamespace::Event,
            key: "deadbeef".to_string(),
            consumer_id: "embed-1".to_string(),
            shape: RefShape::Event(EventShape::Embed),
            liveness: RefLiveness::CacheOk,
        })
    );
}

#[test]
fn ref_dispatch_routes_release_ref() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.release_ref".to_string(),
        payload: serde_json::json!({"namespace": 0, "key": "abc123", "consumer_id": "c"}),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        ref_dispatch_from_action(&action),
        Some(RefDispatch::Release {
            namespace: RefNamespace::Profile,
            key: "abc123".to_string(),
            consumer_id: "c".to_string(),
        })
    );
}

#[test]
fn ref_dispatch_rejects_deleted_event_uri_front_door() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.claim_event".to_string(),
        payload: serde_json::json!({"uri": "nostr:note1abc", "consumer_id": "embed-1"}),
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_action(&action).is_none());
}

#[test]
fn ref_dispatch_returns_none_for_non_ref_type() {
    let action = ActionDispatch {
        action_type: "nmp.publish".to_string(),
        payload: serde_json::json!({}),
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_action(&action).is_none());
}

#[test]
fn ref_dispatch_returns_none_for_missing_field() {
    // Missing consumer_id — defensive parse returns None (D6).
    let action = ActionDispatch {
        action_type: "nmp.kernel.resolve_ref".to_string(),
        payload: serde_json::json!({"namespace": 0, "key": "abc123", "shape": 0, "liveness": 0}),
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_action(&action).is_none());
}

#[test]
fn ref_dispatch_returns_none_for_null_payload() {
    // Payload is null (not a JSON object) — must not panic (D6).
    let action = ActionDispatch {
        action_type: "nmp.kernel.resolve_ref".to_string(),
        payload: serde_json::Value::Null,
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_action(&action).is_none());
}

#[test]
fn ref_dispatch_fails_closed_on_unknown_namespace_discriminant() {
    // namespace = 99 is neither Profile (0) nor Event (1). It MUST reject, NOT
    // coerce to a default namespace (D6, fail-closed).
    let action = ActionDispatch {
        action_type: "nmp.kernel.resolve_ref".to_string(),
        payload: serde_json::json!({
            "namespace": 99, "key": "abc", "consumer_id": "c", "shape": 0, "liveness": 0,
        }),
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_action(&action).is_none());
}

#[test]
fn ref_dispatch_fails_closed_on_unknown_shape_discriminant() {
    // shape = 7 is not a valid profile shape (0 = ref, 1 = card). Reject, never
    // coerce to a default shape (D6).
    let action = ActionDispatch {
        action_type: "nmp.kernel.resolve_ref".to_string(),
        payload: serde_json::json!({
            "namespace": 0, "key": "abc", "consumer_id": "c", "shape": 7, "liveness": 0,
        }),
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_action(&action).is_none());
}

#[test]
fn ref_dispatch_fails_closed_on_unknown_liveness_discriminant() {
    // liveness = 5 is neither CacheOk (0) nor Live (1). Reject (D6).
    let action = ActionDispatch {
        action_type: "nmp.kernel.resolve_ref".to_string(),
        payload: serde_json::json!({
            "namespace": 0, "key": "abc", "consumer_id": "c", "shape": 0, "liveness": 5,
        }),
        correlation_id: "x".to_string(),
    };
    assert!(ref_dispatch_from_action(&action).is_none());
}

#[test]
fn write_path_unavailable_reason_distinguishes_signer_states() {
    // No active account → "sign in to publish".
    assert!(write_path_unavailable_reason(false).starts_with("signer_not_installed"));
    // Account seeded but the action arrived on the JSON dispatch path rather
    // than the typed `dispatch_bytes` doorway (#1008 / ADR-0064). App-level
    // writes MUST cross the binary envelope. The reason token signals this to
    // the host. Legacy tokens (`publish_not_supported_in_web_preview`,
    // `publish_path_not_wired`) are gone.
    let reason = write_path_unavailable_reason(true);
    assert!(
        reason.starts_with("use_dispatch_bytes"),
        "account-seeded JSON-dispatch fallthrough must emit use_dispatch_bytes token; got: {reason}"
    );
    assert!(
        !reason.starts_with("publish_not_supported_in_web_preview"),
        "legacy publish_not_supported_in_web_preview token must be gone; got: {reason}"
    );
    assert!(
        !reason.starts_with("publish_path_not_wired"),
        "legacy publish_path_not_wired token must be gone; got: {reason}"
    );
}

#[test]
fn kernel_action_routes_kernel_namespace_only() {
    let dispatch = ActionDispatch {
        action_type: "nmp.kernel.start".to_string(),
        payload: serde_json::Value::Null,
        correlation_id: "x".to_string(),
    };
    assert!(matches!(
        kernel_action_from_dispatch(&dispatch),
        Some(KernelAction::Start)
    ));

    let app = ActionDispatch {
        action_type: "nmp.publish".to_string(),
        payload: serde_json::Value::Null,
        correlation_id: "y".to_string(),
    };
    assert!(kernel_action_from_dispatch(&app).is_none());
}
