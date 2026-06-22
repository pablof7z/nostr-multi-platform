//! Unit tests for the gallery's typed byte-doorway dispatch seam (ADR-0064 /
//! Cut-B, #1756). These exercise the pure pieces (correlation-id mint, the
//! namespace→typed-payload encoder, the result-envelope parser) without a live
//! kernel — the FFI round-trip is covered by the full-composition gate and the
//! shell smoke tests.

use super::*;

use nmp_core::dispatch_envelope::decode_dispatch_envelope;

// ── correlation-id mint ────────────────────────────────────────────────────

#[test]
fn mint_correlation_id_is_non_empty_and_unique() {
    let a = mint_correlation_id();
    let b = mint_correlation_id();
    assert!(!a.is_empty());
    assert!(!b.is_empty());
    assert_ne!(a, b, "the monotone counter must not repeat within a process");
    assert!(a.starts_with("gallery-"));
}

// ── namespace → typed payload encoder ───────────────────────────────────────

/// Every namespace the gallery's default bundle can dispatch must round-trip
/// its canonical body into typed payload bytes (no JSON fallback, no panic).
/// Coverage matches exactly the action modules `nmp_defaults::register_defaults`
/// installs.
#[test]
fn every_dispatched_namespace_encodes_to_typed_payload() {
    let cases: &[(&str, &str)] = &[
        (
            "nmp.publish",
            r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hi","target":"Auto"}}"#,
        ),
        (
            "nmp.publish",
            r#"{"PublishProfile":{"fields":{"name":"alice"}}}"#,
        ),
        (
            "nmp.nip25.react",
            r#"{"target_event_id":"abc","reaction":"+"}"#,
        ),
        ("nmp.follow", r#"{"pubkey":"deadbeef"}"#),
        ("nmp.unfollow", r#"{"pubkey":"deadbeef"}"#),
        (
            "nmp.nip17.send",
            r#"{"recipient_pubkey":"deadbeef","content":"hello"}"#,
        ),
        (
            "nmp.nip17.publish_relay_list",
            r#"{"relays":["wss://relay.example"]}"#,
        ),
        (
            "nmp.nip57.zap",
            r#"{"recipient_pubkey":"deadbeef","amount_msats":1000}"#,
        ),
        (
            "nmp.nip65.publish_relay_list",
            r#"{"relays":[{"url":"wss://relay.example","role":"read,write"}]}"#,
        ),
        (
            "nmp.nip51.block_relay",
            r#"{"url":"wss://relay.example","account_pubkey":"deadbeef"}"#,
        ),
        (
            "nmp.nip51.unblock_relay",
            r#"{"url":"wss://relay.example","account_pubkey":"deadbeef"}"#,
        ),
    ];

    for (namespace, body) in cases {
        let bytes = super::encode_payload_for_namespace(namespace, body)
            .unwrap_or_else(|e| panic!("namespace {namespace} failed to encode: {e}"));
        assert!(
            !bytes.is_empty(),
            "namespace {namespace} produced empty payload bytes"
        );
    }
}

#[test]
fn unknown_namespace_is_rejected_fail_closed() {
    let err = super::encode_payload_for_namespace("nmp.nope", "{}").unwrap_err();
    assert!(err.contains("no typed payload encoder"));
}

/// A namespace the default bundle does NOT install (NIP-29 groups) is rejected
/// fail-closed — the gallery cannot dispatch it, so there is no encoder.
#[test]
fn undispatchable_namespace_is_rejected_fail_closed() {
    let err = super::encode_payload_for_namespace("nmp.nip29.join", "{}").unwrap_err();
    assert!(err.contains("no typed payload encoder"));
}

#[test]
fn malformed_body_is_rejected_fail_closed() {
    // `nmp.follow` expects `{"pubkey":…}`; a body missing the field is rejected
    // before any envelope is built.
    let err = super::encode_payload_for_namespace("nmp.follow", "{}").unwrap_err();
    assert!(err.contains("does not match its typed payload shape"));
}

/// The encoder's bytes must wrap into a well-formed open envelope that the
/// kernel-side decoder accepts (file id + schema version + namespace +
/// correlation id all present).
#[test]
fn encoded_payload_wraps_into_decodable_envelope() {
    let payload =
        super::encode_payload_for_namespace("nmp.follow", r#"{"pubkey":"deadbeef"}"#).unwrap();
    let corr = mint_correlation_id();
    let envelope = nmp_core::dispatch_envelope::encode_dispatch_envelope(
        &corr,
        "nmp.follow",
        nmp_core::dispatch_envelope::DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    let decoded = decode_dispatch_envelope(&envelope).expect("envelope must decode");
    assert_eq!(decoded.correlation_id, corr);
    assert_eq!(decoded.action_namespace, "nmp.follow");
    assert_eq!(decoded.payload, payload);
}

// ── result-envelope parser ──────────────────────────────────────────────────

#[test]
fn parse_dispatch_envelope_success() {
    let value = serde_json::json!({"correlation_id": "abc123"});
    assert_eq!(parse_dispatch_envelope(&value), Ok("abc123".to_string()));
}

#[test]
fn parse_dispatch_envelope_error() {
    let value = serde_json::json!({"error": "bad action"});
    assert_eq!(
        parse_dispatch_envelope(&value),
        Err("bad action".to_string())
    );
}

#[test]
fn parse_dispatch_envelope_missing_correlation_id() {
    let value = serde_json::json!({"ok": true});
    assert_eq!(
        parse_dispatch_envelope(&value),
        Err("action dispatch envelope missing correlation_id".to_string())
    );
}

#[test]
fn parse_dispatch_envelope_empty_correlation_id() {
    let value = serde_json::json!({"correlation_id": ""});
    assert_eq!(
        parse_dispatch_envelope(&value),
        Err("action dispatch envelope missing correlation_id".to_string())
    );
}
