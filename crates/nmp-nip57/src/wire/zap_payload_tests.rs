//! Round-trip + fail-closed tests for the nip57 zap typed payload codec
//! (ADR-0064 / S9 #1747).

use super::{zap_generated::nmp::nip_57 as fb, SCHEMA_VERSION};
use crate::action::ZapInput;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

#[test]
fn zap_round_trips_full() {
    let action = ZapInput {
        recipient_pubkey: "a".repeat(64),
        amount_msats: 21_000,
        lnurl: Some("https://example.com/lnurl".to_string()),
        relays: vec![
            "wss://relay.example.com".to_string(),
            "wss://nostr.example.com".to_string(),
        ],
        target_event_id: Some("b".repeat(64)),
        comment: Some("🤙 zap".to_string()),
    };
    let decoded = ZapInput::decode(&action.encode()).expect("round-trip decodes");
    assert_eq!(decoded, action);
}

#[test]
fn zap_round_trips_minimal() {
    // Minimal case: no optional fields.
    let action = ZapInput {
        recipient_pubkey: "c".repeat(64),
        amount_msats: 1_000,
        lnurl: None,
        relays: vec![],
        target_event_id: None,
        comment: None,
    };
    let decoded = ZapInput::decode(&action.encode()).expect("round-trip decodes");
    assert_eq!(decoded, action);
    assert!(decoded.lnurl.is_none());
    assert!(decoded.relays.is_empty());
    assert!(decoded.target_event_id.is_none());
    assert!(decoded.comment.is_none());
}

#[test]
fn zap_round_trips_with_relays_no_target() {
    let action = ZapInput {
        recipient_pubkey: "d".repeat(64),
        amount_msats: 5_000,
        lnurl: None,
        relays: vec!["wss://relay.damus.io".to_string()],
        target_event_id: None,
        comment: Some("nice post".to_string()),
    };
    let decoded = ZapInput::decode(&action.encode()).expect("round-trip decodes");
    assert_eq!(decoded, action);
}

/// An explicitly-present but empty `lnurl` string must survive the encode→decode
/// round-trip as `Some("")`, not collapse to `None`. Field presence preservation
/// ensures `ZapAction::start` can reject the empty value rather than the decode
/// layer silently bypassing validation.
#[test]
fn empty_lnurl_preserves_presence_through_round_trip() {
    let action = ZapInput {
        recipient_pubkey: "a".repeat(64),
        amount_msats: 1_000,
        lnurl: Some("".to_string()), // explicitly empty — start() must reject this
        relays: vec![],
        target_event_id: None,
        comment: None,
    };
    let decoded = ZapInput::decode(&action.encode()).expect("round-trip decodes");
    // Presence MUST be preserved: decode returns Some(""), not None.
    assert_eq!(
        decoded.lnurl.as_deref(),
        Some(""),
        "empty lnurl must survive as Some(\"\"), not collapse to None"
    );
}

#[test]
fn wrong_schema_version_is_rejected() {
    // Hand-build a ZapPayload with a bogus schema_version.
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let recipient_pubkey = fbb.create_string(&"a".repeat(64));
    let payload = fb::ZapPayload::create(
        &mut fbb,
        &fb::ZapPayloadArgs {
            schema_version: 999,
            recipient_pubkey: Some(recipient_pubkey),
            amount_msats: 1_000,
            lnurl: None,
            relays: None,
            target_event_id: None,
            comment: None,
        },
    );
    fb::finish_zap_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();

    let err = ZapInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch { found: 999, expected: SCHEMA_VERSION }
    );
}

#[test]
fn malformed_buffers_are_rejected() {
    assert!(matches!(
        ZapInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        ZapInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
