//! Tests for `dispatch_routing` — claim/release parsing, PR-3 interest
//! dispatch parsing, and the stable reason strings.
//!
//! Moved out of `dispatch_routing.rs` (NMP #169-style file-size split) to
//! keep the module root under the 500-LOC hard ceiling.

use super::*;

#[test]
fn claim_dispatch_from_action_routes_claim_profile() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.claim_profile".to_string(),
        payload: serde_json::json!({"pubkey": "abc123", "consumer_id": "chirp-web-author-1"}),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        claim_dispatch_from_action(&action),
        Some(ClaimDispatch::ClaimProfile {
            pubkey: "abc123".to_string(),
            consumer_id: "chirp-web-author-1".to_string(),
        })
    );
}

#[test]
fn claim_dispatch_from_action_routes_release_profile() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.release_profile".to_string(),
        payload: serde_json::json!({"pubkey": "abc123", "consumer_id": "chirp-web-author-1"}),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        claim_dispatch_from_action(&action),
        Some(ClaimDispatch::ReleaseProfile {
            pubkey: "abc123".to_string(),
            consumer_id: "chirp-web-author-1".to_string(),
        })
    );
}

#[test]
fn claim_dispatch_from_action_routes_claim_event() {
    let uri = "nostr:note1abc".to_string();
    let action = ActionDispatch {
        action_type: "nmp.kernel.claim_event".to_string(),
        payload: serde_json::json!({"uri": uri, "consumer_id": "chirp-web-embed-1"}),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        claim_dispatch_from_action(&action),
        Some(ClaimDispatch::ClaimEvent {
            uri,
            consumer_id: "chirp-web-embed-1".to_string(),
        })
    );
}

#[test]
fn claim_dispatch_from_action_routes_release_event() {
    let uri = "nostr:note1abc".to_string();
    let action = ActionDispatch {
        action_type: "nmp.kernel.release_event".to_string(),
        payload: serde_json::json!({"uri": uri, "consumer_id": "chirp-web-embed-1"}),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        claim_dispatch_from_action(&action),
        Some(ClaimDispatch::ReleaseEvent {
            uri,
            consumer_id: "chirp-web-embed-1".to_string(),
        })
    );
}

#[test]
fn claim_dispatch_from_action_returns_none_for_non_claim_type() {
    let action = ActionDispatch {
        action_type: "nmp.publish".to_string(),
        payload: serde_json::json!({}),
        correlation_id: "x".to_string(),
    };
    assert!(claim_dispatch_from_action(&action).is_none());
}

#[test]
fn claim_dispatch_from_action_returns_none_for_missing_field() {
    // Missing consumer_id — defensive parse returns None (D6).
    let action = ActionDispatch {
        action_type: "nmp.kernel.claim_profile".to_string(),
        payload: serde_json::json!({"pubkey": "abc123"}),
        correlation_id: "x".to_string(),
    };
    assert!(claim_dispatch_from_action(&action).is_none());
}

#[test]
fn claim_dispatch_from_action_returns_none_for_null_payload() {
    // Payload is null (not a JSON object) — must not panic (D6).
    let action = ActionDispatch {
        action_type: "nmp.kernel.claim_profile".to_string(),
        payload: serde_json::Value::Null,
        correlation_id: "x".to_string(),
    };
    assert!(claim_dispatch_from_action(&action).is_none());
}

#[test]
fn write_path_unavailable_reason_distinguishes_signer_states() {
    assert!(write_path_unavailable_reason(None).starts_with("signer_not_installed"));
    // Build a real Arc<dyn Signer> using the NIP-07 stub so we exercise
    // the `Some` arm honestly. The signer's sign() will return
    // Unsupported on native; we never call sign() here.
    use nmp_signers::Nip07Signer;
    let signer: Arc<dyn Signer> = Arc::new(Nip07Signer::from_cached_pubkey(
        nostr::PublicKey::from_hex(
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
        )
        .unwrap(),
    ));
    assert!(write_path_unavailable_reason(Some(&signer)).starts_with("publish_path_not_wired"));
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

// ── PR-3 interest dispatch parsing ────────────────────────────────────────────

#[test]
fn interest_dispatch_parses_open_interest() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.open_interest".to_string(),
        payload: serde_json::json!({
            "filter_json": r#"{"kinds":[1,6],"authors":["abc"]}"#,
            "consumer_id": "chirp-web-home",
            "scope": 0,
        }),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        interest_dispatch_from_action(&action),
        Some(InterestDispatch::OpenInterest {
            filter_json: r#"{"kinds":[1,6],"authors":["abc"]}"#.to_string(),
            consumer_id: "chirp-web-home".to_string(),
            scope: 0,
        })
    );
}

#[test]
fn interest_dispatch_parses_close_interest() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.close_interest".to_string(),
        payload: serde_json::json!({
            "filter_json": r#"{"kinds":[1]}"#,
            "consumer_id": "chirp-web-home",
        }),
        correlation_id: "x".to_string(),
    };
    // Missing `scope` defaults to 0.
    assert_eq!(
        interest_dispatch_from_action(&action),
        Some(InterestDispatch::CloseInterest {
            filter_json: r#"{"kinds":[1]}"#.to_string(),
            consumer_id: "chirp-web-home".to_string(),
            scope: 0,
        })
    );
}

#[test]
fn interest_dispatch_parses_open_contact_feed() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.open_contact_feed".to_string(),
        payload: serde_json::json!({"kinds": [1]}),
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        interest_dispatch_from_action(&action),
        Some(InterestDispatch::OpenContactFeed {
            kinds: [1u32, 6u32].into_iter().collect(),
        })
    );
}

#[test]
fn interest_dispatch_rejects_malformed_contact_feed_kind() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.open_contact_feed".to_string(),
        payload: serde_json::json!({"kinds": [4294967296u64]}),
        correlation_id: "x".to_string(),
    };

    assert!(interest_dispatch_from_action(&action).is_none());
}

#[test]
fn interest_dispatch_rejects_repost_wrappers_as_primary_contact_feed_kinds() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.open_contact_feed".to_string(),
        payload: serde_json::json!({"kinds": [1, 6]}),
        correlation_id: "x".to_string(),
    };
    assert!(
        interest_dispatch_from_action(&action).is_none(),
        "apps must declare primary kinds only; kind 6 is derived from primary kind 1"
    );

    let action = ActionDispatch {
        action_type: "nmp.kernel.open_contact_feed".to_string(),
        payload: serde_json::json!({"kinds": [16]}),
        correlation_id: "x".to_string(),
    };
    assert!(
        interest_dispatch_from_action(&action).is_none(),
        "kind 16 is derived for non-kind-1 primary feeds, never declared as primary"
    );
}

#[test]
fn interest_dispatch_parses_close_contact_feed() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.close_contact_feed".to_string(),
        payload: serde_json::Value::Null,
        correlation_id: "x".to_string(),
    };
    assert_eq!(
        interest_dispatch_from_action(&action),
        Some(InterestDispatch::CloseContactFeed)
    );
}

#[test]
fn interest_dispatch_returns_none_for_unknown_type() {
    let action = ActionDispatch {
        action_type: "nmp.kernel.claim_profile".to_string(),
        payload: serde_json::json!({}),
        correlation_id: "x".to_string(),
    };
    assert!(interest_dispatch_from_action(&action).is_none());
}

#[test]
fn interest_dispatch_open_interest_missing_filter_json_returns_none() {
    // D6: missing required field → None, not a panic.
    let action = ActionDispatch {
        action_type: "nmp.kernel.open_interest".to_string(),
        payload: serde_json::json!({"consumer_id": "c", "scope": 0}),
        correlation_id: "x".to_string(),
    };
    assert!(interest_dispatch_from_action(&action).is_none());
}
