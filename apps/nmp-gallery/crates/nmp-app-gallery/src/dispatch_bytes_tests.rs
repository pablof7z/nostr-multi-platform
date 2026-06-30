//! Unit tests for the gallery's typed byte-doorway dispatch seam (ADR-0064 /
//! Cut-B, #1756). These exercise the pure pieces (correlation-id mint, the
//! namespace→typed-payload encoder, the result-envelope parser) without a live
//! kernel — the FFI round-trip is covered by the full-composition gate and the
//! shell smoke tests.
//!
//! ## Load-bearing decode verification (Fix 2, #1843)
//!
//! Every per-namespace test encodes the canonical body through the seam AND
//! decodes the resulting bytes back through the matching typed
//! [`ActionPayload::decode`]. A test that only asserts `!bytes.is_empty()` would
//! pass even if a namespace were accidentally mapped to the WRONG payload type
//! (the encoder would succeed but produce bytes the correct decoder rejects).
//! Decode-and-field-check catches that class of bug at unit-test time, before
//! an FFI dispatch.

use super::*;

use nmp_core::dispatch_envelope::decode_dispatch_envelope;
use nmp_core::substrate::ActionPayload;

// ── correlation-id mint ────────────────────────────────────────────────────

#[test]
fn mint_correlation_id_is_non_empty_and_unique() {
    let a = mint_correlation_id();
    let b = mint_correlation_id();
    assert!(!a.is_empty());
    assert!(!b.is_empty());
    assert_ne!(
        a, b,
        "the monotone counter must not repeat within a process"
    );
    assert!(a.starts_with("gallery-"));
}

// ── helpers ────────────────────────────────────────────────────────────────

/// Encode `json` through the seam for `namespace`, decode the resulting bytes
/// as `P`, and return the decoded value. Panics with a descriptive message on
/// any failure so per-namespace tests stay one-liner asserts.
fn encode_then_decode<P>(namespace: &str, json: &str) -> P
where
    P: ActionPayload,
{
    let bytes = super::encode_payload_for_namespace(namespace, json)
        .unwrap_or_else(|e| panic!("namespace '{namespace}' failed to encode: {e}"));
    assert!(
        !bytes.is_empty(),
        "namespace '{namespace}' produced empty payload bytes"
    );
    P::decode(&bytes).unwrap_or_else(|e| {
        panic!(
            "namespace '{namespace}': decoded bytes do not match payload type {}: {e:?}",
            std::any::type_name::<P>()
        )
    })
}

#[test]
fn namespace_publish_raw_encodes_and_decodes() {
    use action_payloads::PublishAction;
    let decoded: PublishAction = encode_then_decode(
        "nmp.publish",
        r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hi","target":"Auto"}}"#,
    );
    // PublishRaw variant must survive the round-trip.
    assert!(
        matches!(decoded, PublishAction::PublishRaw { .. }),
        "expected PublishAction::PublishRaw, got {decoded:?}"
    );
}

#[test]
fn namespace_publish_profile_encodes_and_decodes() {
    use action_payloads::PublishAction;
    let decoded: PublishAction = encode_then_decode(
        "nmp.publish",
        r#"{"PublishProfile":{"fields":{"name":"alice"}}}"#,
    );
    assert!(
        matches!(decoded, PublishAction::PublishProfile { .. }),
        "expected PublishAction::PublishProfile, got {decoded:?}"
    );
}

#[test]
fn namespace_react_encodes_and_decodes() {
    use action_payloads::ReactAction;
    let decoded: ReactAction = encode_then_decode(
        "nmp.nip25.react",
        r#"{"target_event_id":"abc","reaction":"+"}"#,
    );
    assert_eq!(decoded.target_event_id, "abc");
    assert_eq!(decoded.reaction, "+");
}

#[test]
fn namespace_unreact_encodes_and_decodes() {
    use action_payloads::UnreactAction;
    let decoded: UnreactAction =
        encode_then_decode("nmp.nip25.unreact", r#"{"reaction_event_id":"deadbeef"}"#);
    assert_eq!(decoded.reaction_event_id, "deadbeef");
}

#[test]
fn namespace_follow_encodes_and_decodes() {
    use action_payloads::PubkeyAction;
    let decoded: PubkeyAction = encode_then_decode("nmp.follow", r#"{"pubkey":"deadbeef"}"#);
    assert_eq!(decoded.pubkey, "deadbeef");
}

#[test]
fn namespace_unfollow_encodes_and_decodes() {
    use action_payloads::PubkeyAction;
    let decoded: PubkeyAction = encode_then_decode("nmp.unfollow", r#"{"pubkey":"deadbeef"}"#);
    assert_eq!(decoded.pubkey, "deadbeef");
}

#[test]
fn namespace_follow_many_encodes_and_decodes() {
    use action_payloads::FollowManyAction;
    let decoded: FollowManyAction =
        encode_then_decode("nmp.follow_many", r#"{"pubkeys":["deadbeef","cafebabe"]}"#);
    assert_eq!(decoded.pubkeys, vec!["deadbeef", "cafebabe"]);
}

#[test]
fn namespace_nip17_send_encodes_and_decodes() {
    use action_payloads::SendDmInput;
    let decoded: SendDmInput = encode_then_decode(
        "nmp.nip17.send",
        r#"{"recipient_pubkey":"deadbeef","content":"hello"}"#,
    );
    assert_eq!(decoded.recipient_pubkey, "deadbeef");
    assert_eq!(decoded.content, "hello");
}

#[test]
fn namespace_nip17_publish_relay_list_encodes_and_decodes() {
    use action_payloads::PublishDmRelayListInput;
    let decoded: PublishDmRelayListInput = encode_then_decode(
        "nmp.nip17.publish_relay_list",
        r#"{"relays":["wss://relay.example"]}"#,
    );
    assert_eq!(decoded.relays, vec!["wss://relay.example"]);
}

#[test]
fn namespace_nip51_add_bookmark_encodes_and_decodes() {
    use action_payloads::BookmarkUpdateInput;
    let decoded: BookmarkUpdateInput = encode_then_decode(
        "nmp.nip51.add_bookmark",
        r#"{"account_pubkey":"deadbeef","item":{"type":"url","url":"https://example.com"}}"#,
    );
    assert_eq!(decoded.account_pubkey, "deadbeef");
}

#[test]
fn namespace_nip51_remove_bookmark_encodes_and_decodes() {
    use action_payloads::BookmarkUpdateInput;
    let decoded: BookmarkUpdateInput = encode_then_decode(
        "nmp.nip51.remove_bookmark",
        r#"{"account_pubkey":"deadbeef","item":{"type":"hashtag","hashtag":"nostr"}}"#,
    );
    assert_eq!(decoded.account_pubkey, "deadbeef");
}

#[test]
fn namespace_replies_reply_encodes_and_decodes() {
    use action_payloads::ReplyAction;
    let decoded: ReplyAction = encode_then_decode(
        "nmp.replies.reply",
        r#"{"target_address":"30023:deadbeef:note","target_kind":30023,"content":"great post"}"#,
    );
    assert_eq!(
        decoded.target_address.as_deref(),
        Some("30023:deadbeef:note")
    );
    assert_eq!(decoded.target_kind, 30023);
    assert_eq!(decoded.content, "great post");
}

#[test]
fn namespace_nip65_publish_relay_list_encodes_and_decodes() {
    use action_payloads::PublishRelayListInput;
    let decoded: PublishRelayListInput = encode_then_decode(
        "nmp.nip65.publish_relay_list",
        r#"{"relays":[{"url":"wss://relay.example","role":"read,write"}]}"#,
    );
    assert_eq!(decoded.relays.len(), 1);
    assert_eq!(decoded.relays[0].url, "wss://relay.example");
}

#[test]
fn namespace_nip51_block_relay_encodes_and_decodes() {
    use action_payloads::BlockRelayInput;
    let decoded: BlockRelayInput = encode_then_decode(
        "nmp.nip51.block_relay",
        r#"{"url":"wss://relay.example","account_pubkey":"deadbeef"}"#,
    );
    assert_eq!(decoded.url, "wss://relay.example");
    assert_eq!(decoded.account_pubkey, "deadbeef");
}

#[test]
fn namespace_nip51_unblock_relay_encodes_and_decodes() {
    use action_payloads::UnblockRelayInput;
    let decoded: UnblockRelayInput = encode_then_decode(
        "nmp.nip51.unblock_relay",
        r#"{"url":"wss://relay.example","account_pubkey":"deadbeef"}"#,
    );
    assert_eq!(decoded.url, "wss://relay.example");
    assert_eq!(decoded.account_pubkey, "deadbeef");
}

// ── fail-closed / error-path tests ─────────────────────────────────────────

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
