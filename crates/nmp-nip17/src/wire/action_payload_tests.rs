//! Round-trip + fail-closed tests for the nip17 action typed payload codecs
//! (ADR-0064 / S9 #1747). Every fail-closed gate asserts the NEGATIVE.

use super::*;
use crate::action::{HydratePeerRelayListInput, SendDmInput};
use crate::dm_relay_list::PublishDmRelayListInput;
use nmp_core::substrate::{ActionPayload, ActionPayloadDecodeError};

const RECIPIENT: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

// --- SendDmInput round-trips --------------------------------------------------

#[test]
fn send_dm_round_trips_without_reply_to() {
    let action = SendDmInput {
        recipient_pubkey: RECIPIENT.to_string(),
        content: "hello world".to_string(),
        reply_to: None,
    };
    let decoded = SendDmInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn send_dm_round_trips_with_reply_to() {
    let reply_id = "cc112233445566778899aabbccddeeff00112233445566778899aabbccddee00";
    let action = SendDmInput {
        recipient_pubkey: RECIPIENT.to_string(),
        content: "replying to you".to_string(),
        reply_to: Some(reply_id.to_string()),
    };
    let decoded = SendDmInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn send_dm_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let recipient = fbb.create_string(RECIPIENT);
    let content = fbb.create_string("hello");
    let payload = send_fb::SendDmPayload::create(
        &mut fbb,
        &send_fb::SendDmPayloadArgs {
            schema_version: 999,
            recipient_pubkey: Some(recipient),
            content: Some(content),
            reply_to: None,
        },
    );
    send_fb::finish_send_dm_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = SendDmInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 999,
            expected: SCHEMA_VERSION
        }
    );
}

// --- PublishDmRelayListInput round-trips -------------------------------------

#[test]
fn publish_relay_list_round_trips() {
    let action = PublishDmRelayListInput {
        relays: vec![
            "wss://relay.example".to_string(),
            "wss://inbox.example".to_string(),
        ],
    };
    let decoded = PublishDmRelayListInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn publish_relay_list_empty_round_trips() {
    let action = PublishDmRelayListInput { relays: vec![] };
    let decoded = PublishDmRelayListInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn publish_relay_list_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let r = fbb.create_string("wss://relay.example");
    let relays = fbb.create_vector(&[r]);
    let payload = relay_list_fb::PublishDmRelayListPayload::create(
        &mut fbb,
        &relay_list_fb::PublishDmRelayListPayloadArgs {
            schema_version: 2,
            relays: Some(relays),
        },
    );
    relay_list_fb::finish_publish_dm_relay_list_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = PublishDmRelayListInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 2,
            expected: SCHEMA_VERSION
        }
    );
}

// --- HydratePeerRelayListInput round-trips ----------------------------------

#[test]
fn hydrate_peer_relay_list_round_trips() {
    let action = HydratePeerRelayListInput {
        peer_pubkey: RECIPIENT.to_string(),
    };
    let decoded = HydratePeerRelayListInput::decode(&action.encode()).expect("decodes");
    assert_eq!(decoded, action);
}

#[test]
fn hydrate_peer_relay_list_wrong_schema_version_is_rejected() {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let peer = fbb.create_string(RECIPIENT);
    let payload = hydrate_fb::HydratePeerRelayListPayload::create(
        &mut fbb,
        &hydrate_fb::HydratePeerRelayListPayloadArgs {
            schema_version: 2,
            peer_pubkey: Some(peer),
        },
    );
    hydrate_fb::finish_hydrate_peer_relay_list_payload_buffer(&mut fbb, payload);
    let bytes = fbb.finished_data().to_vec();
    let err = HydratePeerRelayListInput::decode(&bytes).expect_err("bad version rejected");
    assert_eq!(
        err,
        ActionPayloadDecodeError::SchemaVersionMismatch {
            found: 2,
            expected: SCHEMA_VERSION
        }
    );
}

// --- Malformed buffers -------------------------------------------------------

#[test]
fn malformed_buffers_are_rejected() {
    assert!(matches!(
        SendDmInput::decode(b"junk"),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        PublishDmRelayListInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
    assert!(matches!(
        HydratePeerRelayListInput::decode(&[]),
        Err(ActionPayloadDecodeError::Malformed { .. })
    ));
}
